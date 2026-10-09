//! The controlled low-level namespace, exposed to Python as `image.magick`.
//!
//! # Why this exists
//!
//! The high-level API is deliberately Pillow-shaped and therefore lossy: it
//! exposes `mode`, `depth`, `compression`, and nothing else. ImageMagick has far
//! more surface than that, and Stage 01 only opens a *foundation* for it rather
//! than trying to mirror hundreds of MagickWand functions.
//!
//! `image.magick` is that foundation:
//!
//! * raw, unprojected image information (ImageMagick's own image type, the
//!   numeric colorspace/compression/filter values);
//! * encoder settings Pillow has no equivalent for (`with_option`,
//!   `with_compression`, `with_compression_quality`);
//! * information about the linked ImageMagick build.
//!
//! # Semantics
//!
//! `image.magick` is a **live, read-only view** of the image for getters, and
//! **functional** for setters: like the high-level API, every `with_*` method
//! returns a new `Image` rather than mutating in place. That keeps a single
//! immutability rule across the whole library.
//!
//! Adding further MagickWand functionality later means adding methods here; it
//! never requires changing the Python-facing high-level API.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use magik_core::Image;

use crate::exception::IntoPyResult;
use crate::guard;
use crate::image::PyImage;

/// Low-level ImageMagick access for a single image.
#[pyclass(name = "MagickHandle", module = "magik")]
pub struct MagickHandle {
    /// The image this handle reads from and derives new images from.
    owner: Py<PyImage>,
}

impl MagickHandle {
    /// Builds a handle that reads from and derives images from `owner`.
    pub(crate) fn new(owner: Py<PyImage>) -> Self {
        Self { owner }
    }

    /// Borrows the underlying core image.
    fn with_image<T>(&self, py: Python<'_>, f: impl FnOnce(&Image) -> T) -> T {
        f(self.owner.bind(py).borrow().inner())
    }

    /// Applies a functional `with_*` operation, re-wrapping the result.
    fn derive_image(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&Image) -> magik_core::Result<Image>,
    ) -> PyResult<Py<PyImage>> {
        let new_image = self.with_image(py, |image| f(image).py())?;
        Py::new(py, PyImage::new(new_image))
    }
}

#[pymethods]
impl MagickHandle {
    // -- raw image information ---------------------------------------------

    /// ImageMagick's structural classification, e.g. `"TrueColor"`.
    #[getter]
    fn image_type(&self, py: Python<'_>) -> &'static str {
        self.with_image(py, |image| image.image_type())
    }

    /// Number of channels implied by the image type.
    ///
    /// ImageMagick 7.1's MagickWand has no direct getter for this, so magik
    /// derives it from the image type.
    #[getter]
    fn channels(&self, py: Python<'_>) -> u32 {
        self.with_image(py, |image| image.channels())
    }

    /// Bits of precision per colour channel.
    #[getter]
    fn depth(&self, py: Python<'_>) -> u32 {
        self.with_image(py, |image| image.depth())
    }

    /// Colorspace name, e.g. `"sRGB"`.
    #[getter]
    fn colorspace(&self, py: Python<'_>) -> &'static str {
        self.with_image(py, |image| image.colorspace())
    }

    /// Raw numeric `ColorspaceType` value.
    #[getter]
    fn colorspace_value(&self, py: Python<'_>) -> u32 {
        self.with_image(py, |image| image.colorspace_raw())
    }

    /// Compression scheme name, e.g. `"Zip"`.
    #[getter]
    fn compression(&self, py: Python<'_>) -> &'static str {
        self.with_image(py, |image| image.compression())
    }

    /// Raw numeric `CompressionType` value.
    #[getter]
    fn compression_value(&self, py: Python<'_>) -> u32 {
        self.with_image(py, |image| image.compression_raw())
    }

    /// Encoder quality hint in `0..=100`.
    #[getter]
    fn compression_quality(&self, py: Python<'_>) -> u32 {
        self.with_image(py, |image| image.compression_quality())
    }

    /// ImageMagick's format name, e.g. `"PNG"`.
    #[getter]
    fn format(&self, py: Python<'_>) -> PyResult<String> {
        self.with_image(py, |image| image.format().py())
    }

    /// Every metadata field as a dict.
    fn info(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let meta = self.with_image(py, |image| image.metadata());
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
        Ok(dict.unbind())
    }

    // -- encoder settings ---------------------------------------------------

    /// Reads an ImageMagick coder option, e.g. `"png:bit-depth"`.
    ///
    /// Returns `None` when the option is unset.
    fn get_option(&self, py: Python<'_>, key: &str) -> Option<String> {
        self.with_image(py, |image| image.option(key))
    }

    /// Returns a new image carrying the coder option `key = value`.
    fn with_option(&self, py: Python<'_>, key: &str, value: &str) -> PyResult<Py<PyImage>> {
        guard(|| self.derive_image(py, |image| image.with_option(key, value)))
    }

    /// Returns a new image that encodes with the given compression scheme name.
    fn with_compression(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyImage>> {
        guard(|| self.derive_image(py, |image| image.with_compression(name)))
    }

    /// Returns a new image that encodes with the given quality (`0..=100`).
    fn with_compression_quality(&self, py: Python<'_>, quality: u32) -> PyResult<Py<PyImage>> {
        guard(|| self.derive_image(py, |image| image.with_compression_quality(quality)))
    }

    /// Returns a new image in the named colorspace.
    fn with_colorspace(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyImage>> {
        guard(|| self.derive_image(py, |image| image.with_colorspace(name)))
    }

    // -- build information --------------------------------------------------

    /// Version information for the linked ImageMagick build.
    #[getter]
    fn version(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        version_dict(py).map(|dict| dict.unbind())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "<magik.MagickHandle type={:?} size={}x{}>",
            self.image_type(py),
            self.with_image(py, |i| i.width()),
            self.with_image(py, |i| i.height()),
        ))
    }
}

/// Builds the ImageMagick build-information dict used by
/// `image.magick.version` and the module-level `magik.version()`.
pub(crate) fn version_dict(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let version = magik_core::imagemagick_version();
    let dict = PyDict::new(py);
    dict.set_item("version", version.version)?;
    dict.set_item("quantum_depth", version.quantum_depth)?;
    dict.set_item("quantum_range", version.quantum_range)?;
    Ok(dict)
}

/// Module-level helper: names accepted by `with_colorspace`.
#[pyfunction]
pub fn colorspaces() -> PyResult<Py<PyList>> {
    Python::attach(|py| {
        let list = PyList::empty(py);
        for name in [
            "CMYK",
            "CMY",
            "Gray",
            "LinearGray",
            "HSL",
            "HSV",
            "HSB",
            "HWB",
            "Lab",
            "LCH",
            "Luv",
            "OHTA",
            "Rec601YCbCr",
            "Rec709YCbCr",
            "RGB",
            "scRGB",
            "sRGB",
            "xyY",
            "XYZ",
            "YCbCr",
            "YCC",
            "YDbDr",
            "YIQ",
            "YPbPr",
            "YUV",
            "Jzazbz",
            "DisplayP3",
            "Adobe98",
            "ProPhoto",
            "Oklab",
            "Oklch",
        ] {
            list.append(name)?;
        }
        Ok(list.unbind())
    })
}

/// Module-level helper: the channel names `extract_channel` accepts.
///
/// Single-letter aliases (`"r"`, `"g"`, `"b"`, `"a"`, `"c"`, `"m"`, `"y"`,
/// `"k"`) are also accepted but are omitted here to keep the list readable.
#[pyfunction]
pub fn channels() -> PyResult<Py<PyList>> {
    Python::attach(|py| {
        let list = PyList::empty(py);
        for name in [
            "red", "green", "blue", "alpha", "cyan", "magenta", "yellow", "black",
        ] {
            list.append(name)?;
        }
        Ok(list.unbind())
    })
}

/// Module-level helper: names accepted by `with_compression`.
#[pyfunction]
pub fn compressions() -> PyResult<Py<PyList>> {
    Python::attach(|py| {
        let list = PyList::empty(py);
        for name in [
            "B44",
            "B44A",
            "BZip",
            "DXT1",
            "DXT3",
            "DXT5",
            "Fax",
            "Group4",
            "JBIG1",
            "JBIG2",
            "JPEG",
            "JPEG2000",
            "LosslessJPEG",
            "LZMA",
            "LZW",
            "No",
            "PiZ",
            "Pxr24",
            "RLE",
            "Zip",
            "ZipS",
            "Zstd",
            "WebP",
            "BC5",
            "BC7",
            "LERC",
        ] {
            list.append(name)?;
        }
        Ok(list.unbind())
    })
}

/// Module-level helper: names accepted by `Image.resize(filter=...)`.
#[pyfunction]
pub fn filters() -> PyResult<Py<PyList>> {
    Python::attach(|py| {
        let list = PyList::empty(py);
        for name in [
            "Point",
            "Box",
            "Triangle",
            "Hermite",
            "Hann",
            "Hamming",
            "Blackman",
            "Gaussian",
            "Quadratic",
            "Cubic",
            "Catrom",
            "Mitchell",
            "Jinc",
            "Sinc",
            "SincFast",
            "Kaiser",
            "Welch",
            "Parzen",
            "Bohman",
            "Bartlett",
            "Lagrange",
            "Lanczos",
            "LanczosSharp",
            "Lanczos2",
            "Lanczos2Sharp",
            "Robidoux",
            "RobidouxSharp",
            "Cosine",
            "Spline",
            "LanczosRadius",
            "CubicSpline",
        ] {
            list.append(name)?;
        }
        Ok(list.unbind())
    })
}
