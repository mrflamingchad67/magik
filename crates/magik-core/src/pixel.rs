//! Pixel-level access and image construction.
//!
//! # What this module guarantees
//!
//! * **No `unsafe`.** Everything routes through [`Wand`]'s validated methods.
//! * **Coordinates are checked here.** ImageMagick does **not** bounds-check
//!   pixel coordinates: an out-of-range export silently returns an unrelated
//!   pixel rather than failing. Every entry point therefore validates before it
//!   touches the wand.
//! * **One FFI call per bulk operation.** `pixels()` and `from_pixels()` use a
//!   single `MagickExportImagePixels` / `MagickImportImagePixels`, never a loop
//!   over `getpixel`.
//!
//! # The channel map
//!
//! ImageMagick's pixel import/export takes a `map` string naming the channels to
//! transfer. `SetPixelChannelMap`, which interprets it, is an internal function
//! with no header declaration, so the accepted spellings were measured against a
//! Q16-HDRI build rather than assumed:
//!
//! | map    | samples | notes                                   |
//! |--------|---------|-----------------------------------------|
//! | `R`    | 1       | red, or the gray/bilevel level          |
//! | `RGB`  | 3       |                                         |
//! | `RGBA` | 4       | alpha is preserved exactly              |
//! | `CMYK` | 4       |                                         |
//!
//! `GRAY` is **not** a valid map despite being the obvious guess: ImageMagick
//! silently reinterprets it as a four-channel CMYK map and then fails with a
//! misleading "color separated image required".
//!
//! # Sample width
//!
//! `Quantum` is `float` on HDRI builds and `unsigned short` otherwise, so its
//! width is not knowable at compile time — and `MagickGetImageDepth` reports the
//! *minimum* precision an image needs (a 16-bit-capable wand can still report
//! `8`). magik therefore never infers sample width from the image depth. Callers
//! choose it explicitly with the `depth` argument: `8` or `16`.

use magik_sys as sys;

use crate::error::{Error, ErrorKind, Result};
use crate::magick::{classify, PixelColor, Wand};

/// A Pillow-style pixel mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelMode {
    /// `1` — one-bit bilevel, level `0` or `255`.
    Bilevel,
    /// `L` — single-channel grayscale.
    Grayscale,
    /// `P` — palette. See the note on [`PixelMode::channels`].
    Palette,
    /// `RGB` — three channels.
    Rgb,
    /// `RGBA` — three channels plus alpha.
    Rgba,
    /// `CMYK` — four channels.
    Cmyk,
}

impl PixelMode {
    /// Every mode magik can construct and read pixels from.
    pub const ALL: &'static [PixelMode] = &[
        PixelMode::Bilevel,
        PixelMode::Grayscale,
        PixelMode::Palette,
        PixelMode::Rgb,
        PixelMode::Rgba,
        PixelMode::Cmyk,
    ];

    /// The Pillow-style mode name, e.g. `"RGB"`.
    pub const fn name(self) -> &'static str {
        match self {
            PixelMode::Bilevel => "1",
            PixelMode::Grayscale => "L",
            PixelMode::Palette => "P",
            PixelMode::Rgb => "RGB",
            PixelMode::Rgba => "RGBA",
            PixelMode::Cmyk => "CMYK",
        }
    }

    /// Parses a Pillow-style mode name. Case-insensitive.
    pub fn from_name(name: &str) -> Result<Self> {
        let key = name.trim().to_ascii_uppercase();
        Self::ALL
            .iter()
            .copied()
            .find(|mode| mode.name() == key || mode.name().eq_ignore_ascii_case(&key))
            .ok_or_else(|| {
                let known: Vec<&str> = Self::ALL.iter().map(|m| m.name()).collect();
                Error::format(format!(
                    "unsupported mode {name:?}; supported modes are {}",
                    known.join(", ")
                ))
            })
    }

    /// The ImageMagick channel map used to move this mode's pixels.
    ///
    /// `Palette` maps to `"RGB"` rather than a single channel: ImageMagick
    /// resolves palette entries before export, so a one-channel read would hand
    /// back the *red component of the palette colour*, which is misleading.
    /// Returning resolved RGB is honest; note that palette **indices** are not
    /// exposed in Stage 02.
    ///
    /// A palette image with genuine transparency is not read as `Palette` at all
    /// — [`mode_for_image`] promotes it to [`PixelMode::Rgba`], because this map
    /// has no way to carry an alpha channel.
    pub const fn channel_map(self) -> &'static str {
        match self {
            PixelMode::Bilevel | PixelMode::Grayscale => "R",
            // Palette images are read as their resolved colour.
            PixelMode::Palette | PixelMode::Rgb => "RGB",
            PixelMode::Rgba => "RGBA",
            PixelMode::Cmyk => "CMYK",
        }
    }

    /// Number of samples transferred per pixel.
    pub const fn channels(self) -> usize {
        match self {
            PixelMode::Bilevel | PixelMode::Grayscale => 1,
            PixelMode::Palette | PixelMode::Rgb => 3,
            PixelMode::Rgba => 4,
            PixelMode::Cmyk => 4,
        }
    }

    /// Whether a single pixel should surface to Python as one `int` rather than
    /// a tuple. Follows Pillow: single-sample modes are scalars.
    pub const fn is_scalar(self) -> bool {
        self.channels() == 1
    }

    /// The ImageMagick colorspace this mode lives in.
    const fn colorspace(self) -> u32 {
        match self {
            PixelMode::Bilevel | PixelMode::Grayscale => sys::GRAY_COLORSPACE,
            PixelMode::Palette | PixelMode::Rgb | PixelMode::Rgba => sys::SRGB_COLORSPACE,
            PixelMode::Cmyk => sys::CMYK_COLORSPACE,
        }
    }

    /// The ImageMagick image type this mode maps to.
    const fn image_type(self) -> u32 {
        match self {
            PixelMode::Bilevel => sys::BILEVEL_TYPE,
            PixelMode::Grayscale => sys::GRAYSCALE_TYPE,
            PixelMode::Palette => sys::PALETTE_TYPE,
            PixelMode::Rgb => sys::TRUECOLOR_TYPE,
            PixelMode::Rgba => sys::TRUECOLOR_ALPHA_TYPE,
            PixelMode::Cmyk => sys::COLOR_SEPARATION_TYPE,
        }
    }

    /// The largest value a sample of this mode can hold at the given depth.
    pub const fn max_sample(self, bits: u8) -> u32 {
        let _ = self;
        if bits >= 16 {
            65535
        } else {
            255
        }
    }
}

/// Sample width in bits. magik supports 8 and 16.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleDepth {
    /// One byte per sample (`CharPixel`).
    Eight,
    /// Two bytes per sample, native endian (`ShortPixel`).
    Sixteen,
}

impl SampleDepth {
    /// Parses an 8/16 bit depth.
    pub fn from_bits(bits: u8) -> Result<Self> {
        match bits {
            8 => Ok(Self::Eight),
            16 => Ok(Self::Sixteen),
            other => Err(Error::format(format!(
                "unsupported sample depth {other}; magik supports 8 and 16 bits per sample"
            ))),
        }
    }

    /// The depth in bits.
    pub const fn bits(self) -> u8 {
        match self {
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }

    /// Bytes per sample.
    pub const fn bytes(self) -> usize {
        match self {
            Self::Eight => 1,
            Self::Sixteen => 2,
        }
    }

    /// Scales a sample to `bits`, clamping out-of-range values.
    ///
    /// Import accepts an 8-bit buffer for a 16-bit image and vice versa, so
    /// values are rescaled rather than reinterpreted — otherwise a 16-bit image
    /// built from 8-bit data would come out almost black.
    pub fn encode(self, value: u32) -> u32 {
        let value = value.min(self.max_value());
        match self {
            Self::Eight => value,
            Self::Sixteen => value * 257, // 0..255 -> 0..65535, endpoints exact
        }
    }

    /// The largest representable sample at this depth.
    pub const fn max_value(self) -> u32 {
        match self {
            Self::Eight => 255,
            Self::Sixteen => 65535,
        }
    }
}

// ---------------------------------------------------------------------------
// Colour parsing
// ---------------------------------------------------------------------------

/// Parses a `Image.new` colour into an ImageMagick colour specification.
///
/// Accepts a number, a sequence of numbers, or any string ImageMagick
/// understands (`"#ff0000"`, `"red"`, `"rgba(255,0,0,0.5)"`, `"none"`, ...).
///
/// Numbers are **absolute sample values** in `0..=depth.max_value()`, matching
/// Pillow: `Image.new("L", (2, 2), 128)` fills with mid-grey. They are *not*
/// interpreted as `0..1` fractions, because `1` meaning "white" and `1` meaning
/// "almost black" is not a distinction worth making by guesswork. Use a colour
/// string when you want ImageMagick's richer syntax.
pub fn parse_color(spec: &str, mode: PixelMode, depth: SampleDepth) -> Result<String> {
    let text = spec.trim();
    if text.is_empty() {
        return Err(Error::format("colour must not be empty"));
    }

    // A bare number becomes a grey level.
    if let Ok(number) = text.parse::<f64>() {
        return Ok(number_to_color(number, mode, depth));
    }

    // A sequence of numbers becomes an ImageMagick colour list.
    if text.starts_with('[') || text.starts_with('(') {
        let trimmed = text
            .trim_start_matches(['[', '('])
            .trim_end_matches([']', ')']);
        let parts: Vec<f64> = trimmed
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .map(|part| {
                part.trim().parse::<f64>().map_err(|_| {
                    Error::format(format!("colour component {part:?} is not a number"))
                })
            })
            .collect::<Result<Vec<f64>>>()?;
        return numbers_to_color(&parts, mode, depth);
    }

    // Anything else is handed to ImageMagick, which owns colour parsing.
    // Validate eagerly so a bad colour is reported by `Image.new` rather than
    // surfacing later at save time.
    PixelColor::new(text)
        .map_err(|e| classify(ErrorKind::Format, format!("invalid colour {spec:?}"), e))?;
    Ok(text.to_string())
}

/// Renders a single number as an ImageMagick grey level.
fn number_to_color(value: f64, mode: PixelMode, depth: SampleDepth) -> String {
    let max = f64::from(depth.max_value());
    let level = value.clamp(0.0, max).round();
    match mode {
        PixelMode::Bilevel | PixelMode::Grayscale => format!("gray({level})"),
        PixelMode::Palette | PixelMode::Rgb | PixelMode::Rgba => {
            format!("rgb({level},{level},{level})")
        }
        // CMYK is inverted: K = 0 is white.
        PixelMode::Cmyk => format!("cmyk({k},{k},{k},{k})", k = max - level),
    }
}

/// Renders a sequence of numbers as an ImageMagick colour.
fn numbers_to_color(parts: &[f64], mode: PixelMode, depth: SampleDepth) -> Result<String> {
    if parts.is_empty() {
        return Err(Error::format("colour sequence must not be empty"));
    }
    let max = f64::from(depth.max_value());

    let expected = mode.channels();
    if parts.len() != expected {
        return Err(Error::operation(format!(
            "colour has {} component(s) but mode {:?} needs {expected}",
            parts.len(),
            mode.name()
        )));
    }

    let values: Vec<String> = parts
        .iter()
        .map(|value| value.clamp(0.0, max).round().to_string())
        .collect();
    Ok(match mode {
        PixelMode::Bilevel | PixelMode::Grayscale => format!("gray({})", values.join(",")),
        PixelMode::Palette | PixelMode::Rgb => format!("rgb({})", values.join(",")),
        PixelMode::Rgba => {
            // ImageMagick's `rgba()` reads alpha as a **0..1 fraction**, not a
            // 0..255 sample: `rgba(255,0,0,128)` clamps to fully opaque, while
            // `rgba(255,0,0,0.501961)` really is half transparent. Without this
            // conversion an RGBA image built with any alpha below 255 comes out
            // opaque.
            let (rgb, alpha) = values.split_at(values.len() - 1);
            let fraction = alpha[0].parse::<f64>().unwrap_or(0.0) / max;
            format!("rgba({},{})", rgb.join(","), format_alpha(fraction))
        }
        PixelMode::Cmyk => format!("cmyk({})", values.join(",")),
    })
}

/// Formats a 0..1 alpha fraction with enough precision to survive a round trip.
fn format_alpha(fraction: f64) -> String {
    let text = format!("{fraction:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Region transfer helpers
// ---------------------------------------------------------------------------

/// Builds a correctly sized buffer for a region transfer.
pub fn alloc_region(mode: PixelMode, columns: u32, rows: u32, depth: SampleDepth) -> Vec<u8> {
    let samples = columns as usize * rows as usize * mode.channels();
    vec![0u8; samples * depth.bytes()]
}

/// Validates that `(x, y)` addresses a pixel of a `width` x `height` image.
pub fn check_coordinate(x: i64, y: i64, width: u32, height: u32) -> Result<(i64, i64)> {
    if x < 0 || y < 0 {
        return Err(Error::operation(format!(
            "pixel coordinate ({x}, {y}) is negative; coordinates start at (0, 0)"
        )));
    }
    if x >= i64::from(width) || y >= i64::from(height) {
        return Err(Error::operation(format!(
            "pixel coordinate ({x}, {y}) is outside the {width}x{height} image; \
             valid range is (0, 0) to ({}, {})",
            width.saturating_sub(1),
            height.saturating_sub(1)
        )));
    }
    Ok((x, y))
}

/// Exports a whole-image region as raw samples.
///
/// Returns `Err` only when ImageMagick itself fails; buffer sizing is handled by
/// the caller through [`alloc_region`].
pub fn export_region(
    wand: &Wand,
    mode: PixelMode,
    width: u32,
    height: u32,
    depth: SampleDepth,
) -> Result<Vec<u8>> {
    let mut buffer = alloc_region(mode, width, height, depth);
    let context = format!(
        "could not read {}x{} {} pixels at {}-bit depth",
        width,
        height,
        mode.name(),
        depth.bits()
    );
    match depth {
        SampleDepth::Eight => wand
            .export_pixels_u8(0, 0, width, height, mode.channel_map(), &mut buffer)
            .map_err(|e| classify(ErrorKind::Operation, context, e))?,
        SampleDepth::Sixteen => {
            let mut samples = vec![0u16; buffer.len() / 2];
            wand.export_pixels_u16(0, 0, width, height, mode.channel_map(), &mut samples)
                .map_err(|e| classify(ErrorKind::Operation, context.clone(), e))?;
            // Reinterpret as bytes without a copy beyond the unavoidable widening.
            buffer.clear();
            for sample in samples {
                buffer.extend_from_slice(&sample.to_ne_bytes());
            }
        }
    }
    Ok(buffer)
}

/// Reinterprets packed 16-bit samples as native-endian bytes.
pub(crate) fn samples_from_bytes(bytes: &[u8], count: usize) -> Result<Vec<u16>> {
    if bytes.len() < count * 2 {
        return Err(Error::operation(format!(
            "pixel data holds {} bytes but {count} 16-bit samples need {}",
            bytes.len(),
            count * 2
        )));
    }
    Ok(bytes[..count * 2]
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect())
}

/// Imports a whole-image region from raw samples, validating the length.
pub fn import_region(
    wand: &Wand,
    mode: PixelMode,
    width: u32,
    height: u32,
    depth: SampleDepth,
    data: &[u8],
) -> Result<()> {
    let expected_samples = width as usize * height as usize * mode.channels();
    let expected_bytes = expected_samples * depth.bytes();
    if data.len() != expected_bytes {
        return Err(Error::operation(format!(
            "expected {expected_bytes} bytes for a {width}x{height} {} image at {}-bit \
             depth ({} samples x {} bytes), but got {}",
            mode.name(),
            depth.bits(),
            expected_samples,
            depth.bytes(),
            data.len()
        )));
    }
    let context = format!(
        "could not write {}x{} {} pixels",
        width,
        height,
        mode.name()
    );
    match depth {
        SampleDepth::Eight => wand
            .import_pixels_u8(0, 0, width, height, mode.channel_map(), data)
            .map_err(|e| classify(ErrorKind::Operation, context, e)),
        SampleDepth::Sixteen => {
            let samples = samples_from_bytes(data, expected_samples)?;
            wand.import_pixels_u16(0, 0, width, height, mode.channel_map(), &samples)
                .map_err(|e| classify(ErrorKind::Operation, context, e))
        }
    }
}

/// Reads one pixel as a sequence of samples.
pub fn read_pixel(
    wand: &Wand,
    mode: PixelMode,
    x: i64,
    y: i64,
    depth: SampleDepth,
) -> Result<Vec<u32>> {
    let channels = mode.channels();
    match depth {
        SampleDepth::Eight => {
            let mut buffer = vec![0u8; channels];
            wand.export_pixels_u8(x, y, 1, 1, mode.channel_map(), &mut buffer)
                .map_err(|e| {
                    classify(
                        ErrorKind::Operation,
                        format!("could not read pixel ({x}, {y})"),
                        e,
                    )
                })?;
            Ok(buffer.into_iter().map(u32::from).collect())
        }
        SampleDepth::Sixteen => {
            let mut samples = vec![0u16; channels];
            wand.export_pixels_u16(x, y, 1, 1, mode.channel_map(), &mut samples)
                .map_err(|e| {
                    classify(
                        ErrorKind::Operation,
                        format!("could not read pixel ({x}, {y})"),
                        e,
                    )
                })?;
            Ok(samples.into_iter().map(u32::from).collect())
        }
    }
}

/// Writes one pixel from a sequence of samples, at the layer's own error type.
///
/// The channel count and value ranges are validated here rather than being left
/// to ImageMagick, which reports a generic failure for both.
pub fn write_pixel(
    wand: &Wand,
    mode: PixelMode,
    x: i64,
    y: i64,
    depth: SampleDepth,
    samples: &[u32],
) -> Result<()> {
    if samples.len() != mode.channels() {
        return Err(Error::operation(format!(
            "expected {} sample(s) for mode {:?} but got {}",
            mode.name(),
            mode.channels(),
            samples.len()
        )));
    }
    let max = depth.max_value();
    for (index, value) in samples.iter().enumerate() {
        if *value > max {
            return Err(Error::operation(format!(
                "sample {index} is {value}, which exceeds the {}-bit maximum of {max}",
                depth.bits()
            )));
        }
    }
    // Scale so an 8-bit value written to a 16-bit image keeps its brightness.
    let encoded: Vec<u32> = samples.iter().map(|value| depth.encode(*value)).collect();
    let context = format!("could not write pixel ({x}, {y})");
    match depth {
        SampleDepth::Eight => {
            let bytes: Vec<u8> = encoded.iter().map(|value| *value as u8).collect();
            wand.import_pixels_u8(x, y, 1, 1, mode.channel_map(), &bytes)
                .map_err(|e| classify(ErrorKind::Operation, context, e))
        }
        SampleDepth::Sixteen => {
            let words: Vec<u16> = encoded.iter().map(|value| *value as u16).collect();
            wand.import_pixels_u16(x, y, 1, 1, mode.channel_map(), &words)
                .map_err(|e| classify(ErrorKind::Operation, context, e))
        }
    }
}

/// Creates a wand holding a fresh `width` x `height` image of `mode`.
///
/// `depth` is accepted for symmetry with the rest of the pixel API but is not
/// applied to the blank canvas: an ImageMagick image created here reports the
/// quantum depth its contents actually need, which is set by the first pixel
/// import rather than declared up front.
///
/// The call order matters and was established empirically: `MagickNewImage` has
/// to create the image *first*, because both `MagickSetImageColorspace` and
/// `MagickImportImagePixels` fail with "wand contains no images" otherwise.
pub fn create_image(
    mode: PixelMode,
    width: u32,
    height: u32,
    _depth: SampleDepth,
    color: &str,
) -> Result<Wand> {
    if width == 0 || height == 0 {
        return Err(Error::operation(format!(
            "invalid image size {width}x{height}: width and height must be greater than 0"
        )));
    }
    let wand = Wand::new().map_err(|e| {
        classify(
            ErrorKind::Internal,
            "could not allocate an ImageMagick wand",
            e,
        )
    })?;

    let background = PixelColor::new(color)
        .map_err(|e| classify(ErrorKind::Format, format!("invalid colour {color:?}"), e))?;
    wand.new_image(width, height, &background).map_err(|e| {
        classify(
            ErrorKind::Operation,
            format!("could not create a {width}x{height} image"),
            e,
        )
    })?;

    wand.set_colorspace(mode.colorspace()).map_err(|e| {
        classify(
            ErrorKind::Operation,
            "could not set the image colorspace",
            e,
        )
    })?;
    wand.set_image_type(mode.image_type())
        .map_err(|e| classify(ErrorKind::Operation, "could not set the image type", e))?;
    if mode == PixelMode::Palette {
        // Palette needs a quantised colour map to read through.
        wand.set_image_type(sys::TRUECOLOR_TYPE)
            .map_err(|e| classify(ErrorKind::Operation, "could not prepare the image", e))?;
    }
    if mode == PixelMode::Rgba {
        // `MagickNewImage` produces an opaque image, so setting the type alone
        // leaves `has_alpha()` false and the mode reads back as RGB. The alpha
        // channel has to be activated explicitly.
        wand.set_alpha_channel(sys::ACTIVATE_ALPHA_CHANNEL)
            .map_err(|e| {
                classify(
                    ErrorKind::Operation,
                    "could not activate the alpha channel",
                    e,
                )
            })?;
    }
    Ok(wand)
}

/// Which [`PixelMode`] an existing image should be read as.
///
/// Palette images read as resolved RGB rather than as palette indices, matching
/// [`PixelMode::channel_map`].
///
/// A palette image that carries real transparency — ImageMagick reports those as
/// `PaletteAlpha` — is read as **RGBA instead**. Resolving such an image to RGB
/// would silently discard the transparency: a half-transparent pixel would come
/// back looking opaque, and a fully transparent one would read as opaque black.
/// ImageMagick only keeps an alpha channel when some pixel is genuinely not
/// opaque, so keying off the image type is exactly the right signal: an all
/// opaque `Palette` image stays 3-sample RGB with nothing lost.
pub fn mode_for_image(image_type: u32, has_alpha: bool) -> Result<PixelMode> {
    let mode = match image_type {
        sys::BILEVEL_TYPE => PixelMode::Bilevel,
        sys::GRAYSCALE_TYPE | sys::GRAYSCALE_ALPHA_TYPE => {
            // Gray plus transparency is read as RGBA: `PixelMode` has no 2-sample
            // form, and dropping the alpha would report a see-through pixel as
            // opaque. ImageMagick reports the distinction in the type itself, so
            // it needs no help from `has_alpha`.
            if has_alpha || image_type == sys::GRAYSCALE_ALPHA_TYPE {
                PixelMode::Rgba
            } else {
                PixelMode::Grayscale
            }
        }
        sys::PALETTE_TYPE => PixelMode::Palette,
        sys::PALETTE_ALPHA_TYPE | sys::PALETTE_BILEVEL_ALPHA_TYPE => PixelMode::Rgba,
        sys::TRUECOLOR_TYPE | sys::OPTIMIZE_TYPE => {
            if has_alpha {
                PixelMode::Rgba
            } else {
                PixelMode::Rgb
            }
        }
        sys::TRUECOLOR_ALPHA_TYPE => PixelMode::Rgba,
        sys::COLOR_SEPARATION_TYPE | sys::COLOR_SEPARATION_ALPHA_TYPE => PixelMode::Cmyk,
        other => {
            return Err(Error::operation(format!(
                "pixel access is not supported for image type {} ({}); \
                 convert the image to a supported mode first",
                other,
                crate::magick::image_type_name(other)
            )))
        }
    };
    Ok(mode)
}
