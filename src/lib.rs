//! The `magik._magik` CPython extension module.
//!
//! # Layering
//!
//! ```text
//! Python  ->  magik (python/magik)  ->  this crate (PyO3)
//!                                      ->  magik-core
//!                                          ->  magik-sys
//!                                              ->  MagickWand
//! ```
//!
//! This crate is the *only* layer that knows about Python. It performs argument
//! coercion, panic containment and error translation, and deliberately contains
//! no image logic: everything lives in `magik-core`, which itself never mentions
//! Python. That is what keeps the high-level API from being coupled to MagickWand
//! and what would let the backend be replaced without rewriting this file.
//!
//! # Safety guarantees
//!
//! * No raw `MagickWand` pointer is ever exposed to Python.
//! * No Rust panic can cross the boundary: every public entry point runs inside
//!   [`guard`], which converts a panic into a `MagikInternalError`.
//! * No subprocess is spawned; all work is in-process MagickWand calls.

use std::panic::{catch_unwind, AssertUnwindSafe};

use pyo3::prelude::*;
use pyo3::types::PyDict;

pub mod exception;
pub mod image;
pub mod lowlevel;
pub mod pixel;

use image::PyImage;
use lowlevel::{colorspaces, compressions, filters, version_dict};

/// Runs `f`, converting any Rust panic into a `MagikInternalError`.
///
/// This is the single choke point that enforces "no panic crosses PyO3".
pub(crate) fn guard<T>(f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => Err(exception::internal_panic()),
    }
}

/// `magik.open(fp)` — shorthand for `magik.Image.open(fp)`.
#[pyfunction]
fn open(source: &Bound<'_, PyAny>) -> PyResult<Py<PyImage>> {
    guard(|| {
        let image = PyImage::open_source(source)?;
        Py::new(source.py(), image)
    })
}

/// `magik.version()` — information about the linked ImageMagick build.
#[pyfunction]
fn version(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    guard(|| version_dict(py))
}

/// Default resampling filter name, exposed for discoverability.
#[pyfunction]
fn default_filter() -> &'static str {
    magik_core::filter_name(magik_core::DEFAULT_FILTER)
}

/// The pixel modes magik's pixel API understands.
#[pyfunction]
fn modes() -> Vec<&'static str> {
    pixel::MODES.to_vec()
}

/// The extension module.
///
/// Named `_magik` because maturin installs it as `magik._magik`; the public
/// names are re-exported by `python/magik/__init__.py`.
#[pymodule]
fn _magik(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;

    // Exception classes live in `magik.exceptions`; resolve them before anything
    // that could raise one.
    exception::register()?;
    module.add("MagikError", exception::base_class()?)?;

    // High-level API.
    module.add_class::<PyImage>()?;
    module.add_class::<lowlevel::MagickHandle>()?;
    module.add_function(wrap_pyfunction!(open, module)?)?;
    module.add_function(wrap_pyfunction!(version, module)?)?;
    module.add_function(wrap_pyfunction!(default_filter, module)?)?;

    // Low-level foundation.
    module.add_function(wrap_pyfunction!(colorspaces, module)?)?;
    module.add_function(wrap_pyfunction!(compressions, module)?)?;
    module.add_function(wrap_pyfunction!(filters, module)?)?;
    module.add_function(wrap_pyfunction!(modes, module)?)?;

    // Safety net: should an internal invariant still fail, fail as a normal
    // magik exception rather than a raw CPython crash.
    module.add(
        "__all__",
        vec![
            "Image",
            "MagickHandle",
            "open",
            "version",
            "default_filter",
            "colorspaces",
            "compressions",
            "filters",
        ],
    )?;

    Ok(())
}
