//! Python-facing `Image` class.
//!
//! Thin adapter over [`magik_core::Image`]: it does argument coercion, panic
//! containment and error translation, and nothing else. No ImageMagick detail
//! and no raw pointer appears here.

use std::path::PathBuf;

use pyo3::buffer::PyBuffer;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use magik_core::{Image, PixelMode};

use crate::exception::{to_py_error, type_error, IntoPyResult};
use crate::guard;
use crate::pixel::{
    coerce_buffer, coerce_pixel_value, coerce_size_2d, coerce_xy, parse_depth, samples_to_object,
};

/// Where an image is read from.
enum Source {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

/// Where an image is written to.
enum Target<'py> {
    Path(PathBuf),
    File(Bound<'py, PyAny>),
}

/// A decoded image.
///
/// Operations are **immutable**: every geometry/colour method returns a new
/// `Image` and leaves the receiver untouched. See the README for the rationale.
#[pyclass(name = "Image", module = "magik")]
pub struct PyImage {
    inner: Image,
}

impl PyImage {
    pub(crate) fn new(inner: Image) -> Self {
        Self { inner }
    }

    /// Decodes an image from any supported source.
    ///
    /// Shared by the `Image.open` static method and the module-level
    /// `magik.open` shortcut so both accept exactly the same inputs.
    pub(crate) fn open_source(source: &Bound<'_, PyAny>) -> PyResult<Self> {
        match coerce_source(source)? {
            Source::Path(path) => Image::open(path),
            Source::Bytes(data) => Image::from_bytes(&data),
        }
        .map(Self::new)
        .py()
    }

    pub(crate) fn inner(&self) -> &Image {
        &self.inner
    }
}

/// Coerces `source` into a filesystem path or an in-memory buffer.
///
/// Accepts, in priority order:
/// 1. a file-like object with a callable `read()` (e.g. `io.BytesIO`);
/// 2. any object supporting the buffer protocol (`bytes`, `bytearray`,
///    `memoryview`);
/// 3. a `str` or `os.PathLike` path.
fn coerce_source(obj: &Bound<'_, PyAny>) -> PyResult<Source> {
    // 1. file-like
    if let Ok(read) = obj.getattr("read") {
        if read.is_callable() {
            let data = read.call0()?;
            return match PyBuffer::<u8>::get(&data) {
                Ok(buffer) => Ok(Source::Bytes(buffer.to_vec(obj.py())?)),
                Err(_) => Err(type_error(
                    "file object .read() must return a bytes-like object",
                )),
            };
        }
    }

    // 2. buffer protocol: bytes, bytearray, memoryview, array, ...
    if let Ok(buffer) = PyBuffer::<u8>::get(obj) {
        return Ok(Source::Bytes(buffer.to_vec(obj.py())?));
    }

    // 3. path (str or os.PathLike)
    match obj.extract::<PathBuf>() {
        Ok(path) => Ok(Source::Path(path)),
        Err(_) => Err(type_error(format!(
            "cannot open image from {}: expected a path, a bytes-like object, or a file-like object",
            obj.get_type().name()?
        ))),
    }
}

/// Coerces `target` into a filesystem path or a file-like object.
fn coerce_target<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Target<'py>> {
    if let Ok(write) = obj.getattr("write") {
        if write.is_callable() {
            return Ok(Target::File(obj.clone()));
        }
    }
    match obj.extract::<PathBuf>() {
        Ok(path) => Ok(Target::Path(path)),
        Err(_) => Err(type_error(format!(
            "cannot save image to {}: expected a path or a writable file-like object",
            obj.get_type().name()?
        ))),
    }
}

#[pymethods]
impl PyImage {
    /// Opens an image from a path, a bytes-like object or a file-like object.
    ///
    /// ```python
    /// Image.open("input.jpg")
    /// Image.open(io.BytesIO(data))
    /// Image.open(raw_bytes)
    /// ```
    #[staticmethod]
    fn open(source: &Bound<'_, PyAny>) -> PyResult<Self> {
        guard(|| PyImage::open_source(source))
    }

    /// Decodes an image from an in-memory buffer.
    ///
    /// Accepts `bytes`, `bytearray`, `memoryview` and anything else exporting the
    /// buffer protocol. The data is passed to ImageMagick without an
    /// intermediate decode through Python objects beyond the one copy needed to
    /// hand it to Rust.
    #[staticmethod]
    fn from_bytes(data: &Bound<'_, PyAny>) -> PyResult<Self> {
        guard(|| {
            let buffer = coerce_buffer(data)?;
            Image::from_bytes(&buffer).map(Self::new).py()
        })
    }

    // -- construction ------------------------------------------------------

    /// Creates a new image of `mode` and `size`, filled with `color`.
    ///
    /// ```python
    /// Image.new("RGB", (640, 480), "black")
    /// Image.new("L", (64, 64), 128)            # mid-grey
    /// Image.new("RGBA", (10, 10), (255, 0, 0, 128))
    /// ```
    ///
    /// `mode` is one of `"1"`, `"L"`, `"P"`, `"RGB"`, `"RGBA"` or `"CMYK"`.
    /// `color` may be a number, a sequence of numbers, or any string ImageMagick
    /// understands (`"#ff0000"`, `"red"`, `"rgba(255,0,0,0.5)"`, `"none"`).
    /// Numbers are absolute sample values in `0..=255` (or `0..=65535` at
    /// `depth=16`), matching Pillow.
    ///
    /// The result is MIFF-backed: lossless and always available. `image.format`
    /// reports `"MIFF"` until the image is written.
    #[staticmethod]
    #[pyo3(name = "new", signature = (mode, size, color = None, *, depth = None))]
    fn new_image(
        mode: &str,
        size: &Bound<'_, PyAny>,
        color: Option<&Bound<'_, PyAny>>,
        depth: Option<u8>,
    ) -> PyResult<Self> {
        guard(|| {
            let mode = PixelMode::from_name(mode).py()?;
            let size = coerce_size_2d(size)?;
            let depth = parse_depth(depth)?;
            let color = match color {
                Some(value) if !value.is_none() => color_to_spec(value)?,
                _ => "black".to_string(),
            };
            Image::new(mode, size, &color, depth).map(Self::new).py()
        })
    }

    /// Builds an image directly from packed pixel samples.
    ///
    /// ```python
    /// Image.from_pixels("RGB", (2, 2), bytes([255,0,0,  0,255,0,  0,0,255,  255,255,0]))
    /// ```
    ///
    /// `data` must be exactly `width * height * channels * (depth // 8)` bytes.
    /// Samples are interleaved in row-major order.
    #[staticmethod]
    #[pyo3(signature = (mode, size, data, *, depth = None))]
    fn from_pixels(
        mode: &str,
        size: &Bound<'_, PyAny>,
        data: &Bound<'_, PyAny>,
        depth: Option<u8>,
    ) -> PyResult<Self> {
        guard(|| {
            let mode = PixelMode::from_name(mode).py()?;
            let size = coerce_size_2d(size)?;
            let depth = parse_depth(depth)?;
            let data = coerce_buffer(data)?;
            Image::from_pixels(mode, size, &data, depth)
                .map(Self::new)
                .py()
        })
    }

    // -- pixel access ------------------------------------------------------

    /// The pixel mode this image is read as.
    #[getter]
    fn pixel_mode(&self) -> PyResult<&'static str> {
        guard(|| Ok(self.inner.pixel_mode().py()?.name()))
    }

    /// Reads a single pixel.
    ///
    /// Returns an `int` for single-channel modes (`"1"`, `"L"`) and a `tuple`
    /// otherwise, matching Pillow.
    ///
    /// ```python
    /// image.getpixel((0, 0))
    /// ```
    #[pyo3(signature = (xy, *, depth = None))]
    fn getpixel(&self, xy: &Bound<'_, PyAny>, depth: Option<u8>) -> PyResult<Py<PyAny>> {
        guard(|| {
            let (x, y) = coerce_xy(xy)?;
            let depth = parse_depth(depth)?;
            let mode = self.inner.pixel_mode().py()?;
            let samples = self.inner.getpixel(x, y, depth).py()?;
            Ok(samples_to_object(mode, &samples))
        })
    }

    /// Returns a copy of the image with the pixel at `(x, y)` replaced.
    ///
    /// Like every other magik operation this is **immutable**: the receiver is
    /// left untouched and a new image is returned.
    #[pyo3(signature = (xy, value, *, depth = None))]
    fn putpixel(
        &self,
        xy: &Bound<'_, PyAny>,
        value: &Bound<'_, PyAny>,
        depth: Option<u8>,
    ) -> PyResult<Self> {
        guard(|| {
            let (x, y) = coerce_xy(xy)?;
            let depth = parse_depth(depth)?;
            let mode = self.inner.pixel_mode().py()?;
            let samples = coerce_pixel_value(value, mode, depth)?;
            self.inner
                .putpixel(x, y, &samples, depth)
                .map(Self::new)
                .py()
        })
    }

    /// Returns a copy of the image with a rectangular region replaced.
    ///
    /// The region is overwritten, not blended.
    #[pyo3(signature = (origin, size, data, *, depth = None))]
    fn putpixels(
        &self,
        origin: &Bound<'_, PyAny>,
        size: &Bound<'_, PyAny>,
        data: &Bound<'_, PyAny>,
        depth: Option<u8>,
    ) -> PyResult<Self> {
        guard(|| {
            let origin = coerce_xy(origin)?;
            let size = coerce_size_2d(size)?;
            let depth = parse_depth(depth)?;
            let data = coerce_buffer(data)?;
            self.inner
                .putpixels(origin, size, &data, depth)
                .map(Self::new)
                .py()
        })
    }

    /// The whole image as packed pixel samples, as `bytes`.
    ///
    /// This is a **single** ImageMagick call, not a loop over `getpixel`.
    /// Samples are interleaved in row-major order using the image's pixel mode:
    /// 1 sample for `"1"`/`"L"`, 3 for `"RGB"`, 4 for `"RGBA"`/`"CMYK"`.
    ///
    /// `depth=16` returns two bytes per sample in native (little) endian order.
    #[pyo3(signature = (*, depth = None))]
    fn pixels(&self, depth: Option<u8>) -> PyResult<Py<PyBytes>> {
        guard(|| {
            let depth = parse_depth(depth)?;
            let data = self.inner.pixels(depth).py()?;
            Python::attach(|py| Ok(PyBytes::new(py, &data).unbind()))
        })
    }

    /// An independent copy of this image.
    fn copy(&self) -> PyResult<Self> {
        guard(|| self.inner.try_clone().map(Self::new).py())
    }

    // -- metadata ----------------------------------------------------------

    /// Width in pixels.
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width()
    }

    /// Height in pixels.
    #[getter]
    fn height(&self) -> u32 {
        self.inner.height()
    }

    /// `(width, height)`.
    #[getter]
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// ImageMagick's format name, e.g. `"PNG"` or `"JPEG"`.
    #[getter]
    fn format(&self) -> PyResult<String> {
        self.inner.format().py()
    }

    /// Pillow-style mode string, e.g. `"RGB"`. See the README for the mapping.
    #[getter]
    fn mode(&self) -> &'static str {
        self.inner.mode()
    }

    /// Number of colour channels, derived from ImageMagick's image type.
    #[getter]
    fn channels(&self) -> u32 {
        self.inner.channels()
    }

    /// Bits of precision per colour channel (not bits per pixel).
    #[getter]
    fn depth(&self) -> u32 {
        self.inner.depth()
    }

    /// ImageMagick's structural classification, e.g. `"TrueColor"`.
    #[getter]
    fn image_type(&self) -> &'static str {
        self.inner.image_type()
    }

    /// Colorspace name, e.g. `"sRGB"`.
    #[getter]
    fn colorspace(&self) -> &'static str {
        self.inner.colorspace()
    }

    /// Compression scheme name, e.g. `"Zip"`.
    #[getter]
    fn compression(&self) -> &'static str {
        self.inner.compression()
    }

    /// Encoder quality hint in `0..=100`.
    #[getter]
    fn compression_quality(&self) -> u32 {
        self.inner.compression_quality()
    }

    /// The path this image was opened from, or `None`.
    #[getter]
    fn filename(&self) -> Option<String> {
        self.inner.source_path().map(|p| p.display().to_string())
    }

    /// The low-level ImageMagick namespace for this image.
    ///
    /// ```python
    /// image.magick.image_type      # "TrueColor"
    /// image.magick.version         # ImageMagick build information
    /// image2 = image.magick.with_option("jpeg:sampling-factor", "4:4:4")
    /// ```
    #[getter]
    fn magick(slf: Py<Self>) -> PyResult<crate::lowlevel::MagickHandle> {
        Ok(crate::lowlevel::MagickHandle::new(slf))
    }

    /// A dict with every Stage 01 metadata field.
    fn metadata(&self) -> PyResult<Py<PyDict>> {
        let meta = self.inner.metadata();
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("width", meta.width)?;
            dict.set_item("height", meta.height)?;
            dict.set_item("format", meta.format)?;
            dict.set_item("mode", meta.mode)?;
            dict.set_item("channels", meta.channels)?;
            dict.set_item("depth", meta.depth)?;
            dict.set_item("image_type", meta.image_type)?;
            dict.set_item("colorspace", meta.colorspace)?;
            dict.set_item("compression", meta.compression)?;
            dict.set_item("compression_quality", meta.compression_quality)?;
            dict.set_item("filename", meta.source)?;
            Ok(dict.unbind())
        })
    }

    // -- operations --------------------------------------------------------

    /// Resamples the image.
    ///
    /// Accepts `resize(width, height)` or Pillow's `resize((width, height))`.
    ///
    /// `filter` is an ImageMagick resampling kernel name (default `"lanczos"`).
    #[pyo3(signature = (width, height = None, *, filter = None))]
    fn resize(
        &self,
        width: &Bound<'_, PyAny>,
        height: Option<&Bound<'_, PyAny>>,
        filter: Option<&str>,
    ) -> PyResult<Self> {
        guard(|| {
            let (w, h) = coerce_size(width, height)?;
            match filter {
                Some(name) => {
                    let filter = magik_core::filter_by_name(name).py()?;
                    self.inner.resize_with_filter(w, h, filter)
                }
                None => self.inner.resize(w, h),
            }
            .map(Self::new)
            .py()
        })
    }

    /// Extracts the rectangle `(left, top)` .. `(right, bottom)`.
    ///
    /// Accepts `crop(left, top, right, bottom)` or Pillow's
    /// `crop((left, top, right, bottom))`. The box must lie inside the image;
    /// magik does not pad out-of-bounds requests the way Pillow does.
    #[pyo3(signature = (left, top = None, right = None, bottom = None))]
    fn crop(
        &self,
        left: &Bound<'_, PyAny>,
        top: Option<&Bound<'_, PyAny>>,
        right: Option<&Bound<'_, PyAny>>,
        bottom: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        guard(|| {
            let (l, t, r, b) = coerce_box(left, top, right, bottom)?;
            self.inner.crop(l, t, r, b).map(Self::new).py()
        })
    }

    /// Rotates counter-clockwise by `angle` degrees.
    ///
    /// `background` is keyword-only and defaults to transparent.
    ///
    /// The resulting size depends on the angle: multiples of 90 transpose the
    /// image (`0`/`180`/`360` keep the size, `90`/`270` swap width and height),
    /// while any other angle **grows** it, because ImageMagick expands the canvas
    /// so the rotated corners are not clipped. A 6x4 image rotated 45 degrees
    /// therefore comes back as 10x10, not 6x4.
    #[pyo3(signature = (angle, *, background = None))]
    fn rotate(&self, angle: f64, background: Option<&str>) -> PyResult<Self> {
        guard(|| self.inner.rotate(angle, background).map(Self::new).py())
    }

    /// Mirrors the image vertically (top <-> bottom).
    fn flip(&self) -> PyResult<Self> {
        guard(|| self.inner.flip().map(Self::new).py())
    }

    /// Mirrors the image horizontally (left <-> right).
    fn flop(&self) -> PyResult<Self> {
        guard(|| self.inner.flop().map(Self::new).py())
    }

    /// Converts the image to grayscale.
    fn grayscale(&self) -> PyResult<Self> {
        guard(|| self.inner.grayscale().map(Self::new).py())
    }

    // -- Stage 05: colorspace, channels, alpha, precision -------------------

    /// Converts the image into another colorspace, transforming the pixels.
    ///
    /// This is not the same as assigning a colorspace: for an RGB pixel
    /// `(0, 0, 200)`, `convert_colorspace("Gray")` returns the luminance `14`,
    /// whereas treating those same samples as gray yields `0` - the red channel.
    ///
    /// Accepts any name from `magik.colorspaces()`, case-insensitively.
    fn convert_colorspace(&self, name: &str) -> PyResult<Self> {
        guard(|| self.inner.convert_colorspace(name).map(Self::new).py())
    }

    /// Extracts one channel as a new single-channel image.
    ///
    /// Supported names: `red`, `green`, `blue`, `alpha`, and the CMYK roles
    /// `cyan`, `magenta`, `yellow`, `black`. Requesting a channel the image does
    /// not have - `red` from a grayscale image, say - raises `MagikOperationError`.
    fn extract_channel(&self, name: &str) -> PyResult<Self> {
        guard(|| self.inner.extract_channel(name).map(Self::new).py())
    }

    /// Whether the image carries an alpha channel.
    ///
    /// Read from ImageMagick's own alpha state, not inferred from `mode` or
    /// `channels`: a CMYK image has four channels but no alpha.
    #[getter]
    fn has_alpha(&self) -> bool {
        self.inner.has_alpha()
    }

    /// Quantises the image to 8 or 16 bits per sample.
    ///
    /// Reducing the depth discards precision that converting back cannot
    /// recover. This is the image's storage depth and is independent of the
    /// `depth=` argument accepted by `pixels()` and friends.
    fn convert_depth(&self, bits: u32) -> PyResult<Self> {
        guard(|| self.inner.convert_depth(bits).map(Self::new).py())
    }

    /// Applies a Gaussian blur.
    ///
    /// `sigma` defaults to `radius / 2`.
    #[pyo3(signature = (radius, sigma = None))]
    fn blur(&self, radius: f64, sigma: Option<f64>) -> PyResult<Self> {
        guard(|| {
            let sigma = sigma.unwrap_or(radius / 2.0);
            self.inner.blur(radius, sigma).map(Self::new).py()
        })
    }

    // -- output ------------------------------------------------------------

    /// Writes the image.
    ///
    /// `target` may be a path or a writable file-like object. The output format
    /// is taken from `format`, else from the filename extension, else from the
    /// image's current format.
    #[pyo3(signature = (target, format = None))]
    fn save(&self, target: &Bound<'_, PyAny>, format: Option<&str>) -> PyResult<()> {
        guard(|| match coerce_target(target)? {
            Target::Path(path) => match format {
                Some(name) => self.inner.save_with_format(path, name),
                None => self.inner.save(path),
            }
            .py(),
            Target::File(file) => {
                let data = self.inner.write_bytes(format).py()?;
                let payload = PyBytes::new(file.py(), &data);
                file.call_method1("write", (payload,))?;
                Ok(())
            }
        })
    }

    /// Encodes the image and returns the bytes.
    #[pyo3(signature = (format = None))]
    fn to_bytes(&self, format: Option<&str>) -> PyResult<Py<PyBytes>> {
        guard(|| {
            let data = self.inner.write_bytes(format).py()?;
            Python::attach(|py| Ok(PyBytes::new(py, &data).unbind()))
        })
    }

    /// Alias for `to_bytes`, for symmetry with `save`.
    #[pyo3(signature = (format = None))]
    fn save_to_bytes(&self, format: Option<&str>) -> PyResult<Py<PyBytes>> {
        self.to_bytes(format)
    }

    fn __repr__(&self) -> String {
        format!(
            "<magik.Image mode={} size={}x{} format={:?}>",
            self.inner.mode(),
            self.inner.width(),
            self.inner.height(),
            self.inner.format().unwrap_or_default()
        )
    }
}

/// Accepts `resize(800, 600)` or `resize((800, 600))`.
fn coerce_size(
    width: &Bound<'_, PyAny>,
    height: Option<&Bound<'_, PyAny>>,
) -> PyResult<(u32, u32)> {
    if let Some(height) = height {
        return Ok((
            positive_u32(width, "width")?,
            positive_u32(height, "height")?,
        ));
    }
    // Single sequence argument: Pillow style.
    if let Ok((w, h)) = width.extract::<(u32, u32)>() {
        return Ok((w, h));
    }
    Err(type_error(
        "resize() takes resize(width, height) or resize((width, height))",
    ))
}

/// Accepts `crop(0, 0, 50, 40)` or `crop((0, 0, 50, 40))`.
fn coerce_box(
    left: &Bound<'_, PyAny>,
    top: Option<&Bound<'_, PyAny>>,
    right: Option<&Bound<'_, PyAny>>,
    bottom: Option<&Bound<'_, PyAny>>,
) -> PyResult<(i64, i64, i64, i64)> {
    if let (Some(top), Some(right), Some(bottom)) = (top, right, bottom) {
        let pick = |value: &Bound<'_, PyAny>, name: &str| -> PyResult<i64> {
            value
                .extract::<i64>()
                .map_err(|_| type_error(format!("crop {name} must be an integer")))
        };
        return Ok((
            pick(left, "left")?,
            pick(top, "top")?,
            pick(right, "right")?,
            pick(bottom, "bottom")?,
        ));
    }
    match left.extract::<(i64, i64, i64, i64)>() {
        Ok(box_) => Ok(box_),
        Err(_) => Err(type_error(
            "crop() takes crop(left, top, right, bottom) or crop((left, top, right, bottom))",
        )),
    }
}

/// Extracts a strictly positive dimension, so a zero/negative size is reported
/// as a `MagikOperationError` by `magik-core` rather than reaching ImageMagick.
fn positive_u32(value: &Bound<'_, PyAny>, name: &str) -> PyResult<u32> {
    let parsed: i64 = value
        .extract()
        .map_err(|_| type_error(format!("{name} must be an integer")))?;
    u32::try_from(parsed)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| {
            to_py_error(magik_core::Error::operation(format!(
                "invalid {name}: {parsed}"
            )))
        })
}

/// Converts a Python colour argument into an ImageMagick colour specification.
///
/// `int` and sequences become absolute sample lists; a string is passed through
/// for ImageMagick to parse. `magik-core` owns the actual colour semantics.
fn color_to_spec(value: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(number) = value.extract::<i64>() {
        return Ok(number.to_string());
    }
    if let Ok(sequence) = value.extract::<Vec<i64>>() {
        let joined = sequence
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        return Ok(format!("({joined})"));
    }
    if let Ok(text) = value.extract::<String>() {
        return Ok(text);
    }
    // Fall back to ImageMagick's own repr for exotic colour objects.
    value
        .str()
        .map(|text| text.to_string_lossy().into_owned())
        .map_err(|_| {
            to_py_error(magik_core::Error::format(
                "colour must be a number, a sequence of numbers, or a colour string",
            ))
        })
}
