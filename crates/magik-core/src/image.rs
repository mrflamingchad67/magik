//! The safe, high-level image abstraction.
//!
//! This module contains **no `unsafe` code at all** — every FFI call goes
//! through [`crate::magick::Wand`], which owns the raw pointer and confines the
//! unsafety.
//!
//! # Mutation policy
//!
//! magik uses **immutable, functional operations**: geometry and colour
//! operations return a *new* [`Image`] and leave the receiver untouched.
//!
//! ```no_run
//! use magik_core::Image;
//!
//! let image = Image::open("input.jpg")?;
//! let thumb = image.resize(800, 600)?;
//! thumb.grayscale()?.save("output.png")?;
//! # Ok::<(), magik_core::Error>(())
//! ```
//!
//! The trade-off is deliberate:
//!
//! * **Pro** — an `Image` handle is never aliased, so it cannot be mutated
//!   underneath another consumer, and errors can be propagated instead of
//!   accumulated inside a wand.
//! * **Con** — each operation clones the wand. ImageMagick's `CloneMagickWand`
//!   copies the image *structure* and shares the pixel cache, so this is a
//!   structure copy rather than a full pixel-buffer copy. `benches/` measures
//!   the real cost, and later stages can add explicit in-place variants if the
//!   numbers warrant it.
//!
//! This decision is also what makes the low-level settings API
//! ([`Image::with_compression_quality`], [`Image::with_option`]) safe: each
//! derived image owns its own wand, so encoder settings cannot leak between
//! images.
//!
//! # Threading
//!
//! The wand is held behind a [`Mutex`], which is what makes [`Image`] both
//! [`Send`] and [`Sync`] *soundly*: ImageMagick forbids using one wand from two
//! threads at once, and this mutex is the serialisation point. Copies of the
//! image (as produced by [`Image::try_clone`] and every operation) get their own
//! mutex and can therefore be used concurrently on different threads.
//!
//! The mutex is uncontended in the normal case — the Python layer holds the GIL
//! for the whole call — so the cost is a few nanoseconds against operations that
//! take milliseconds.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use magik_sys as sys;
use magik_sys::wand::Result as RawResult;

use crate::error::{Error, ErrorKind, Result};
use crate::magick::{
    channels_for_image_type, classify, colorspace_from_name, colorspace_name,
    compression_from_name, compression_name, filter_from_name, image_type_name, MagickVersion,
    Wand, DEFAULT_FILTER,
};
use crate::pixel::{
    check_coordinate, create_image, export_region, import_region, mode_for_image, parse_color,
    read_pixel, samples_from_bytes, write_pixel, PixelMode, SampleDepth,
};

/// A decoded image, backed by an ImageMagick wand.
///
/// See the [module documentation](self#mutation-policy) for the immutability
/// contract and [module documentation](self#threading) for the threading model.
pub struct Image {
    /// The wand plus the lock that makes concurrent access sound. Never taken
    /// out of the struct, and never handed out by reference.
    wand: Mutex<Wand>,
    /// Path the image was opened from, when applicable. Kept so `save()` can fall
    /// back to the original extension when the output path has none.
    source: Option<PathBuf>,
}

impl Image {
    /// Opens an image from a filesystem path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let wand = Wand::new().map_err(|e| {
            classify(
                ErrorKind::Internal,
                "could not allocate an ImageMagick wand",
                e,
            )
        })?;
        wand.read_path(path).map_err(|e| {
            classify(
                ErrorKind::Open,
                format!("could not open image {:?}", path.display()),
                e,
            )
        })?;
        Ok(Self {
            wand: Mutex::new(wand),
            source: Some(path.to_path_buf()),
        })
    }

    /// Decodes an image from an in-memory buffer.
    ///
    /// The buffer is passed to ImageMagick without copying it.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let wand = Wand::new().map_err(|e| {
            classify(
                ErrorKind::Internal,
                "could not allocate an ImageMagick wand",
                e,
            )
        })?;
        wand.read_blob(data).map_err(|e| {
            classify(
                ErrorKind::Open,
                "could not decode image data (unrecognised or corrupt)",
                e,
            )
        })?;
        Ok(Self {
            wand: Mutex::new(wand),
            source: None,
        })
    }

    /// Wraps an existing wand, taking ownership of it.
    pub fn from_wand(wand: Wand) -> Self {
        Self {
            wand: Mutex::new(wand),
            source: None,
        }
    }

    /// The path this image was opened from, if any.
    pub fn source_path(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Copies the image (wand deep-copy, pixel cache shared).
    pub fn try_clone(&self) -> Result<Self> {
        let wand = self
            .lock()
            .try_clone()
            .map_err(|e| classify(ErrorKind::Internal, "could not copy the image", e))?;
        Ok(Self {
            wand: Mutex::new(wand),
            source: self.source.clone(),
        })
    }

    /// Acquires the wand lock.
    ///
    /// Poisoning is deliberately ignored: every wand access happens inside
    /// [`Wand`]'s safe methods, which never leave the wand half-mutated, so a
    /// panic elsewhere must not render the image permanently unusable.
    fn lock(&self) -> MutexGuard<'_, Wand> {
        self.wand.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` against the wand while holding the lock.
    ///
    /// This is the only supported way to reach the low-level handle: it hands
    /// out a borrow that cannot outlive the lock, so no caller can retain a
    /// `&Wand` (and therefore no raw pointer) after the call returns.
    pub fn with_wand<T>(&self, f: impl FnOnce(&Wand) -> T) -> T {
        f(&self.lock())
    }

    // -- metadata ----------------------------------------------------------

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.lock().width()
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.lock().height()
    }

    /// `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        let guard = self.lock();
        (guard.width(), guard.height())
    }

    /// Bits of precision **per colour channel** (e.g. `8` or `16`).
    ///
    /// This is *not* bits per pixel: a 16-bit RGBA image has depth `16`.
    pub fn depth(&self) -> u32 {
        self.lock().depth()
    }

    /// Number of colour channels, derived from ImageMagick's image type.
    ///
    /// See [`channels_for_image_type`] for why this is derived rather than read.
    pub fn channels(&self) -> u32 {
        channels_for_image_type(self.lock().image_type())
    }

    /// The encoder/decoder name ImageMagick associated with this image, e.g.
    /// `"PNG"`, `"JPEG"`, `"GIF"`.
    pub fn format(&self) -> Result<String> {
        self.lock().format().map_err(|e| {
            classify(
                ErrorKind::Internal,
                "could not determine the image format",
                e,
            )
        })
    }

    /// ImageMagick's structural classification, e.g. `"TrueColor"`.
    pub fn image_type(&self) -> &'static str {
        image_type_name(self.lock().image_type())
    }

    /// A Pillow-style mode string.
    ///
    /// ImageMagick has no direct equivalent of Pillow's `mode`, so this is a
    /// documented *projection* of the image type. Where Pillow keeps an alpha
    /// channel outside the mode string (palette transparency, CMYK alpha) magik
    /// follows Pillow and omits it. See [`mode_for_image_type`] for the table.
    pub fn mode(&self) -> &'static str {
        mode_for_image_type(self.lock().image_type())
    }

    /// The colorspace's canonical ImageMagick name, e.g. `"sRGB"`.
    pub fn colorspace(&self) -> &'static str {
        colorspace_name(self.lock().colorspace())
    }

    /// The raw numeric `ColorspaceType`, for callers that need the ABI value.
    pub fn colorspace_raw(&self) -> u32 {
        self.lock().colorspace()
    }

    /// The compression scheme's canonical name, e.g. `"Zip"` (PNG) or
    /// `"JPEG"`.
    pub fn compression(&self) -> &'static str {
        compression_name(self.lock().compression())
    }

    /// The raw numeric `CompressionType`, for callers that need the ABI value.
    pub fn compression_raw(&self) -> u32 {
        self.lock().compression()
    }

    /// Encoder quality hint (`0..=100`), as ImageMagick reports it.
    pub fn compression_quality(&self) -> u32 {
        self.lock().compression_quality()
    }

    /// A snapshot of everything the Stage 01 API knows about the image.
    pub fn metadata(&self) -> ImageMetadata {
        // One lock acquisition for the whole snapshot, so the fields cannot come
        // from different points in time.
        let guard = self.lock();
        let image_type = guard.image_type();
        ImageMetadata {
            width: guard.width(),
            height: guard.height(),
            format: guard.format().unwrap_or_default(),
            mode: mode_for_image_type(image_type).to_string(),
            channels: channels_for_image_type(image_type),
            depth: guard.depth(),
            image_type: image_type_name(image_type).to_string(),
            colorspace: colorspace_name(guard.colorspace()).to_string(),
            compression: compression_name(guard.compression()).to_string(),
            compression_quality: guard.compression_quality(),
            source: self.source.as_ref().map(|p| p.display().to_string()),
        }
    }

    // -- operations --------------------------------------------------------

    /// Applies `op` to a copy of the wand, returning the modified image.
    ///
    /// This is the single place the copy-on-write behaviour lives. The operation
    /// runs while the receiver is still locked, so a derived image can never
    /// observe a partially mutated parent.
    ///
    /// `op` speaks the raw `magik-sys` error type; `kind` and `context` are the
    /// core-level policy applied on the way out, which keeps every
    /// `WandError` → [`Error`] translation in one place.
    fn derive<F>(&self, kind: ErrorKind, context: impl Into<String>, op: F) -> Result<Self>
    where
        F: FnOnce(&Wand) -> RawResult<()>,
    {
        let guard = self.lock();
        let cloned = guard
            .try_clone()
            .map_err(|e| classify(ErrorKind::Internal, "could not copy the image", e))?;
        op(&cloned).map_err(|e| classify(kind, context, e))?;
        Ok(Self {
            wand: Mutex::new(cloned),
            source: self.source.clone(),
        })
    }

    /// Resamples the image to exactly `width` x `height` using the default
    /// Lanczos filter.
    ///
    /// See [`Image::resize_with_filter`] to choose a specific kernel.
    pub fn resize(&self, width: u32, height: u32) -> Result<Self> {
        self.resize_with_filter(width, height, DEFAULT_FILTER)
    }

    /// Resamples the image to exactly `width` x `height` using `filter`.
    ///
    /// `filter` is an ImageMagick [`FilterType`](crate::magick::filter_name)
    /// value; use [`crate::magick::filter_from_name`] to resolve one from a
    /// string such as `"lanczos"` or `"point"`.
    pub fn resize_with_filter(&self, width: u32, height: u32, filter: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::operation(format!(
                "invalid resize dimensions {width}x{height}: width and height must be greater than 0"
            )));
        }
        self.derive(
            ErrorKind::Operation,
            format!("could not resize image to {width}x{height}"),
            |wand| wand.resize(width, height, filter),
        )
    }

    /// Extracts the rectangle from `(left, top)` to `(right, bottom)`.
    ///
    /// The box uses Pillow's convention: `right`/`bottom` are exclusive, so
    /// cropping a 100x80 image with `(0, 0, 50, 40)` yields a 50x40 image.
    ///
    /// Unlike Pillow, the box must lie inside the image. Pillow silently pads
    /// out-of-bounds requests; magik reports them instead, because padding
    /// requires extra canvas operations that Stage 01 deliberately does not do.
    pub fn crop(&self, left: i64, top: i64, right: i64, bottom: i64) -> Result<Self> {
        let (width, height) = self.size();
        if right <= left || bottom <= top {
            return Err(Error::operation(format!(
                "invalid crop box ({left}, {top}, {right}, {bottom}): \
                 right must be greater than left and bottom greater than top"
            )));
        }
        if left < 0 || top < 0 {
            return Err(Error::operation(format!(
                "invalid crop box ({left}, {top}, {right}, {bottom}): origin must not be negative"
            )));
        }
        if right > i64::from(width) || bottom > i64::from(height) {
            return Err(Error::operation(format!(
                "invalid crop box ({left}, {top}, {right}, {bottom}): \
                 exceeds image bounds {width}x{height}"
            )));
        }
        self.derive(
            ErrorKind::Operation,
            format!("could not crop image to ({left}, {top}, {right}, {bottom})"),
            |wand| wand.crop(left, top, right, bottom),
        )
    }

    /// Rotates the image **counter-clockwise** by `angle` degrees.
    ///
    /// Direction matches Pillow (positive = counter-clockwise), which is the
    /// opposite of ImageMagick's native `MagickRotateImage`; magik negates the
    /// angle internally.
    ///
    /// Corners exposed by a non-multiple-of-90 rotation are filled with
    /// `background` (default: transparent). The canvas is *not* expanded to fit
    /// the rotated image — Pillow's `expand=True` is not supported in Stage 01.
    pub fn rotate(&self, angle: f64, background: Option<&str>) -> Result<Self> {
        if !angle.is_finite() {
            return Err(Error::operation(format!(
                "invalid rotation angle {angle}: must be finite"
            )));
        }
        let fill = background.unwrap_or("none");
        // ImageMagick rotates clockwise; the Pillow-style API rotates CCW.
        self.derive(
            ErrorKind::Operation,
            format!("could not rotate image by {angle} degrees"),
            |wand| wand.rotate(-angle, fill),
        )
    }

    /// Mirrors the image vertically (top ↔ bottom).
    pub fn flip(&self) -> Result<Self> {
        self.derive(ErrorKind::Operation, "could not flip image", Wand::flip)
    }

    /// Mirrors the image horizontally (left ↔ right).
    pub fn flop(&self) -> Result<Self> {
        self.derive(ErrorKind::Operation, "could not flop image", Wand::flop)
    }

    /// Converts the image to grayscale.
    pub fn grayscale(&self) -> Result<Self> {
        self.derive(
            ErrorKind::Operation,
            "could not convert image to grayscale",
            |wand| wand.transform_colorspace(sys::GRAY_COLORSPACE),
        )
    }

    /// Applies a Gaussian blur of `radius` with `sigma` deviation.
    pub fn blur(&self, radius: f64, sigma: f64) -> Result<Self> {
        if !radius.is_finite() || !sigma.is_finite() || radius < 0.0 || sigma < 0.0 {
            return Err(Error::operation(format!(
                "invalid blur parameters radius={radius}, sigma={sigma}: \
                 both must be finite and non-negative"
            )));
        }
        self.derive(
            ErrorKind::Operation,
            format!("could not blur image (radius={radius}, sigma={sigma})"),
            |wand| wand.blur(radius, sigma),
        )
    }

    /// Converts the image to another colorspace, given a name such as `"Gray"`,
    /// `"CMYK"` or `"sRGB"`.
    pub fn with_colorspace(&self, name: &str) -> Result<Self> {
        let value = colorspace_from_name(name)
            .ok_or_else(|| Error::format(format!("unknown colorspace {name:?}")))?;
        self.derive(
            ErrorKind::Operation,
            format!("could not set colorspace {}", colorspace_name(value)),
            |wand| wand.set_colorspace(value),
        )
    }

    // -- encoder settings (low-level) --------------------------------------

    /// Returns a copy that encodes with `quality` (`0..=100`).
    pub fn with_compression_quality(&self, quality: u32) -> Result<Self> {
        self.derive(
            ErrorKind::Operation,
            format!("could not set compression quality to {quality}"),
            |wand| wand.set_compression_quality(quality),
        )
    }

    /// Returns a copy that encodes with the given compression scheme name
    /// (e.g. `"Zip"`, `"JPEG"`, `"WebP"`).
    pub fn with_compression(&self, name: &str) -> Result<Self> {
        let value = compression_from_name(name)
            .ok_or_else(|| Error::format(format!("unknown compression scheme {name:?}")))?;
        self.derive(
            ErrorKind::Operation,
            format!("could not set compression {}", compression_name(value)),
            |wand| wand.set_compression(value),
        )
    }

    /// Returns a copy carrying the coder option `key = value`.
    ///
    /// Options are ImageMagick's own, e.g. `"png:bit-depth"` or
    /// `"jpeg:sampling-factor"`.
    pub fn with_option(&self, key: &str, value: &str) -> Result<Self> {
        self.derive(
            ErrorKind::Operation,
            format!("could not set option {key:?} to {value:?}"),
            |wand| wand.set_option(key, value),
        )
    }

    /// Reads a coder option previously set on this image.
    pub fn option(&self, key: &str) -> Option<String> {
        self.lock().get_option(key)
    }

    // -- output ------------------------------------------------------------

    /// Writes the image to `path`.
    ///
    /// The output format is inferred from the filename extension (`.png` → PNG,
    /// `.jpg` → JPEG, ...), falling back to the format the image was decoded
    /// from when the extension is missing or unrecognised.
    ///
    /// Inferring from the extension is what makes `jpeg_image.save("thumb.png")`
    /// produce a real PNG; without it ImageMagick would write JPEG bytes into a
    /// file named `.png`. See [`Image::save_with_format`] to override.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let target = format_from_path(path);
        self.with_wand(|wand| wand.write_path(path, target.as_deref()))
            .map_err(|e| {
                classify(
                    ErrorKind::Save,
                    format!("could not write image to {:?}", path.display()),
                    e,
                )
            })
    }

    /// Writes the image to `path` in an explicitly named `format`.
    ///
    /// The format takes precedence over the filename extension. It is applied
    /// through ImageMagick's coder prefix (`PNG:out.jpg`) so a misleading
    /// extension cannot override it -- but the format is **validated first**,
    /// because ImageMagick silently falls back to the extension when it does not
    /// recognise a coder prefix. Without that check,
    /// `save("out.png", format="NOT-A-FORMAT")` would quietly write a PNG instead
    /// of reporting the typo.
    pub fn save_with_format<P: AsRef<Path>>(&self, path: P, format: &str) -> Result<()> {
        let path = path.as_ref();
        let target = normalize_format(format)?;
        let guard = self.lock();
        let previous = guard.format().ok();
        validate_format(&guard, &target)?;
        let result = guard
            .write_path(path, Some(&target))
            .and_then(|()| confirm_file_written(path, &target));
        // Always restore, even when the write failed.
        if let Some(previous) = previous {
            let _ = guard.set_format(&previous);
        }
        result.map_err(|e| {
            classify(
                ErrorKind::Save,
                format!("could not write image to {:?}", path.display()),
                e,
            )
        })
    }

    /// Encodes the image into a new byte buffer.
    ///
    /// `format` defaults to the image's current format.
    pub fn write_bytes(&self, format: Option<&str>) -> Result<Vec<u8>> {
        let target = match format {
            Some(name) => Some(normalize_format(name)?),
            None => None,
        };
        self.encode_with(Wand::write_blob, target.as_deref())
    }

    /// Runs `write` with `format` temporarily selected on the wand.
    ///
    /// The wand's original format is restored afterwards, so encoding never
    /// mutates the image's identity.
    fn encode_with<T, F>(&self, write: F, format: Option<&str>) -> Result<T>
    where
        F: FnOnce(&Wand) -> RawResult<T>,
    {
        let context = "could not encode image to a buffer";
        let guard = self.lock();
        let Some(format) = format else {
            return write(&guard).map_err(|e| classify(ErrorKind::Save, context, e));
        };
        let previous = guard.format().ok();
        validate_format(&guard, format)?;
        let result = write(&guard);
        // Always restore, even when the encode failed.
        if let Some(previous) = previous {
            let _ = guard.set_format(&previous);
        }
        result.map_err(|e| classify(ErrorKind::Save, context, e))
    }

    // -- construction ------------------------------------------------------

    /// Creates a new image of `mode` and `size`, filled with `color`.
    ///
    /// `color` may be a number, a single-element sequence, or any string
    /// ImageMagick understands (`"#ff0000"`, `"red"`, `"rgba(255,0,0,0.5)"`,
    /// `"none"`). Numbers and sequences are scaled to `depth`.
    ///
    /// The result is MIFF-backed: it is lossless and always available, and it is
    /// what [`Image::format`] reports until the image is written.
    pub fn new(mode: PixelMode, size: (u32, u32), color: &str, depth: SampleDepth) -> Result<Self> {
        let (width, height) = size;
        let color = parse_color(color, mode, depth)?;
        let wand = create_image(mode, width, height, depth, &color)?;
        // MIFF is ImageMagick's native lossless format, so a freshly built image
        // can be encoded without pretending to be a PNG.
        wand.set_format("MIFF")
            .map_err(|e| classify(ErrorKind::Internal, "could not set the new image format", e))?;
        Ok(Self {
            wand: Mutex::new(wand),
            source: None,
        })
    }

    /// Builds an image directly from packed pixel samples.
    ///
    /// `data` must be exactly `width * height * mode.channels() * depth.bytes()`
    /// bytes; see [`SampleDepth`] for the packing.
    pub fn from_pixels(
        mode: PixelMode,
        size: (u32, u32),
        data: &[u8],
        depth: SampleDepth,
    ) -> Result<Self> {
        let (width, height) = size;
        if width == 0 || height == 0 {
            return Err(Error::operation(format!(
                "invalid image size {width}x{height}: width and height must be greater than 0"
            )));
        }
        let color = if mode.is_scalar() { "gray(0)" } else { "black" };
        let wand = create_image(mode, width, height, depth, color)?;
        import_region(&wand, mode, width, height, depth, data)?;
        wand.set_format("MIFF")
            .map_err(|e| classify(ErrorKind::Internal, "could not set the new image format", e))?;
        Ok(Self {
            wand: Mutex::new(wand),
            source: None,
        })
    }

    // -- pixel access ------------------------------------------------------

    /// The [`PixelMode`] this image's pixels are read as.
    pub fn pixel_mode(&self) -> Result<PixelMode> {
        self.with_wand(|wand| mode_for_image(wand.image_type(), wand.has_alpha()))
    }

    /// Reads a single pixel.
    ///
    /// Returns a one-element `Vec` for single-channel modes and a full sample
    /// list otherwise; the Python layer turns the former into a bare `int`, which
    /// is what Pillow does.
    ///
    /// Coordinates are bounds-checked, because ImageMagick silently returns an
    /// unrelated pixel for out-of-range coordinates rather than failing.
    pub fn getpixel(&self, x: i64, y: i64, depth: SampleDepth) -> Result<Vec<u32>> {
        let (width, height) = self.size();
        let (x, y) = check_coordinate(x, y, width, height)?;
        let mode = self.pixel_mode()?;
        self.with_wand(|wand| read_pixel(wand, mode, x, y, depth))
    }

    /// Returns a copy of the image with the pixel at `(x, y)` replaced.
    ///
    /// Like every other operation, this is **immutable**: the receiver is left
    /// untouched and a new image is returned.
    pub fn putpixel(&self, x: i64, y: i64, samples: &[u32], depth: SampleDepth) -> Result<Self> {
        let (width, height) = self.size();
        let (x, y) = check_coordinate(x, y, width, height)?;
        let mode = self.pixel_mode()?;
        let guard = self.lock();
        let cloned = guard
            .try_clone()
            .map_err(|e| classify(ErrorKind::Internal, "could not copy the image", e))?;
        write_pixel(&cloned, mode, x, y, depth, samples)?;
        Ok(Self {
            wand: Mutex::new(cloned),
            source: self.source.clone(),
        })
    }

    /// Returns a copy of the image with a rectangular region replaced.
    ///
    /// The region is overwritten, not blended.
    pub fn putpixels(
        &self,
        origin: (i64, i64),
        size: (u32, u32),
        data: &[u8],
        depth: SampleDepth,
    ) -> Result<Self> {
        let (image_width, image_height) = self.size();
        let (columns, rows) = size;
        if columns == 0 || rows == 0 {
            return Err(Error::operation(format!(
                "invalid region size {columns}x{rows}: both must be greater than 0"
            )));
        }
        let (x, y) = check_coordinate(origin.0, origin.1, image_width, image_height)?;
        // The whole region must fit; ImageMagick would reject a partial write
        // anyway, and a clear message beats its generic one.
        if x + i64::from(columns) > i64::from(image_width)
            || y + i64::from(rows) > i64::from(image_height)
        {
            return Err(Error::operation(format!(
                "region {columns}x{rows} at ({x}, {y}) does not fit inside the \
                 {image_width}x{image_height} image"
            )));
        }
        let mode = self.pixel_mode()?;
        let expected = columns as usize * rows as usize * mode.channels() * depth.bytes();
        if data.len() != expected {
            return Err(Error::operation(format!(
                "expected {expected} bytes for a {columns}x{rows} {} region but got {}",
                mode.name(),
                data.len()
            )));
        }
        let context = format!("could not write a {columns}x{rows} pixel region");
        // Widen the 16-bit buffer before the closure so the closure stays in the
        // raw error type that `derive` expects.
        let widened = match depth {
            SampleDepth::Eight => None,
            SampleDepth::Sixteen => Some(samples_from_bytes(
                data,
                columns as usize * rows as usize * mode.channels(),
            )?),
        };
        self.derive(ErrorKind::Operation, context, |wand| {
            match (&widened, depth) {
                (None, SampleDepth::Eight) => {
                    wand.import_pixels_u8(x, y, columns, rows, mode.channel_map(), data)
                }
                (Some(samples), SampleDepth::Sixteen) => {
                    wand.import_pixels_u16(x, y, columns, rows, mode.channel_map(), samples)
                }
                // Unreachable: `widened` is `Some` exactly when depth is 16.
                _ => Ok(()),
            }
        })
    }

    /// The whole image's pixels as packed samples.
    ///
    /// Samples are interleaved in row-major order using the image's
    /// [`PixelMode::channel_map`]. This is **one** ImageMagick call, not a loop
    /// over [`Image::getpixel`].
    pub fn pixels(&self, depth: SampleDepth) -> Result<Vec<u8>> {
        let (width, height) = self.size();
        let mode = self.pixel_mode()?;
        self.with_wand(|wand| export_region(wand, mode, width, height, depth))
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("size", &self.size())
            .field("format", &self.format().ok())
            .field("mode", &self.mode())
            .finish()
    }
}

/// A point-in-time snapshot of image metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMetadata {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// ImageMagick format name, e.g. `"PNG"`.
    pub format: String,
    /// Pillow-style mode projection, e.g. `"RGB"`.
    pub mode: String,
    /// Colour channels implied by the image type.
    pub channels: u32,
    /// Bits of precision per channel.
    pub depth: u32,
    /// ImageMagick structural classification, e.g. `"TrueColor"`.
    pub image_type: String,
    /// Colorspace name, e.g. `"sRGB"`.
    pub colorspace: String,
    /// Compression scheme name, e.g. `"Zip"`.
    pub compression: String,
    /// Encoder quality hint in `0..=100`.
    pub compression_quality: u32,
    /// Path the image was opened from, when applicable.
    pub source: Option<String>,
}

/// Checks that ImageMagick recognises `format` as a coder, before it is used.
///
/// `MagickSetImageFormat` is the authoritative test — it is the one call that
/// actually rejects a name it does not know. It is also the one ImageMagick
/// function that reports failure *without* recording a reason, so on failure
/// magik supplies its own explanation rather than passing on a bare C function
/// name that tells the user nothing.
///
/// Validating here matters because ImageMagick's writer degrades silently
/// otherwise: an unrecognised coder prefix falls back to the filename extension,
/// so a typo would write a valid file in the wrong format instead of erroring.
fn validate_format(wand: &Wand, format: &str) -> Result<()> {
    wand.set_format(format).map_err(|error| {
        // Preserve ImageMagick's reason when it gave one; otherwise explain.
        let detail = error.message.clone().or_else(|| {
            Some(format!(
                "ImageMagick does not recognise {format:?} as an image coder. \
                 This build supports PNG, JPEG, WebP, GIF, TIFF, BMP and others; \
                 see the ImageMagick format list for your installation."
            ))
        });
        let mut failure = Error::format(format!("unknown or unusable image format {format:?}"));
        failure = failure.with_detail(detail.unwrap_or_default());
        failure
    })
}

/// Confirms a path-based write actually produced a file.
///
/// Some ImageMagick coders (notably `NULL`) report success while writing
/// nothing at all. Without this check the caller gets a bare
/// `FileNotFoundError` from the next filesystem operation instead of a magik
/// error explaining what happened.
fn confirm_file_written(path: &Path, format: &str) -> magik_sys::wand::Result<()> {
    if path.exists() {
        return Ok(());
    }
    Err(
        magik_sys::wand::WandError::bare(magik_sys::TYPE_ERROR).with_message(format!(
            "the {format} coder reported success but produced no file at {:?}",
            path.display()
        )),
    )
}

/// Normalises a user-supplied format name to ImageMagick's uppercase spelling.
fn normalize_format(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::format("format name must not be empty"));
    }
    if let Some(canonical) = format_from_extension(trimmed) {
        return Ok(canonical.to_string());
    }
    // Unknown names are passed through verbatim: ImageMagick supports far more
    // formats than magik has an alias for, and the real capability check happens
    // when the encoder is invoked. A missing delegate is then reported as
    // `ErrorKind::UnsupportedFormat`.
    Ok(trimmed.to_ascii_uppercase())
}

/// Maps a filename to an ImageMagick format name.
fn format_from_path(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    format_from_extension(extension).map(str::to_string)
}

/// Maps a filename extension to ImageMagick's canonical format name.
///
/// Extensions ImageMagick names differently from the web are mapped here
/// (`jpg` → `JPEG`, `tif` → `TIFF`, ...). Anything absent from the table is
/// treated as an unknown format rather than guessed at, which makes the caller
/// fall back to the image's current format instead of writing a mislabelled file.
fn format_from_extension(extension: &str) -> Option<&'static str> {
    const TABLE: &[(&str, &str)] = &[
        ("png", "PNG"),
        ("jpg", "JPEG"),
        ("jpeg", "JPEG"),
        ("jpe", "JPEG"),
        ("jfif", "JPEG"),
        ("webp", "WEBP"),
        ("gif", "GIF"),
        ("bmp", "BMP"),
        ("dib", "BMP"),
        ("tif", "TIFF"),
        ("tiff", "TIFF"),
        ("ico", "ICO"),
        ("cur", "CUR"),
        ("jp2", "JP2"),
        ("j2k", "JP2"),
        ("jpc", "JPC"),
        ("pdf", "PDF"),
        ("ps", "PS"),
        ("eps", "EPS"),
        ("tga", "TGA"),
        ("pcx", "PCX"),
        ("pbm", "PBM"),
        ("pgm", "PGM"),
        ("ppm", "PPM"),
        ("pnm", "PNM"),
        ("pam", "PAM"),
        ("psd", "PSD"),
        ("svg", "SVG"),
        ("avif", "AVIF"),
        ("heic", "HEIC"),
        ("heif", "HEIF"),
        ("jxl", "JXL"),
        ("dds", "DDS"),
        ("exr", "EXR"),
        ("hdr", "HDR"),
        ("wbmp", "WBMP"),
        ("xpm", "XPM"),
        ("xbm", "XBM"),
        ("miff", "MIFF"),
        ("mpc", "MPC"),
        ("fits", "FITS"),
        ("palm", "PALM"),
        ("pict", "PICT"),
        ("ras", "RAS"),
        ("rgb", "RGB"),
        ("rgba", "RGBA"),
        ("cmyk", "CMYK"),
        ("yuv", "YUV"),
    ];
    let key = extension.trim().to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(ext, _)| *ext == key)
        .map(|(_, format)| *format)
}

/// Pillow-style mode projection of an ImageMagick image type.
///
/// | ImageMagick type                                        | magik mode |
/// |---------------------------------------------------------|------------|
/// | Bilevel                                                 | `1`        |
/// | Grayscale                                               | `L`        |
/// | GrayscaleAlpha                                          | `LA`       |
/// | Palette, PaletteAlpha, PaletteBilevelAlpha             | `P`        |
/// | TrueColor, Optimize                                     | `RGB`      |
/// | TrueColorAlpha                                          | `RGBA`     |
/// | ColorSeparation, ColorSeparationAlpha                  | `CMYK`     |
/// | anything else                                           | `RGB`      |
///
/// `PaletteAlpha` and `ColorSeparationAlpha` collapse to `P` / `CMYK` because
/// Pillow tracks an alpha/transparency channel outside the mode string; magik
/// follows that convention so `mode` stays comparable with Pillow's.
fn mode_for_image_type(value: u32) -> &'static str {
    match value {
        sys::BILEVEL_TYPE => "1",
        sys::GRAYSCALE_TYPE => "L",
        sys::GRAYSCALE_ALPHA_TYPE => "LA",
        sys::PALETTE_TYPE | sys::PALETTE_ALPHA_TYPE | sys::PALETTE_BILEVEL_ALPHA_TYPE => "P",
        sys::TRUECOLOR_TYPE | sys::OPTIMIZE_TYPE => "RGB",
        sys::TRUECOLOR_ALPHA_TYPE => "RGBA",
        sys::COLOR_SEPARATION_TYPE | sys::COLOR_SEPARATION_ALPHA_TYPE => "CMYK",
        // `Undefined` and anything a future ImageMagick adds.
        _ => "RGB",
    }
}

/// Describes the linked ImageMagick build.
pub fn imagemagick_version() -> MagickVersion {
    crate::magick::version()
}

/// Resolves a filter name for callers that prefer strings over numeric codes.
pub fn filter_by_name(name: &str) -> Result<u32> {
    filter_from_name(name)
        .ok_or_else(|| Error::format(format!("unknown resampling filter {name:?}")))
}
