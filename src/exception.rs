//! The Python exception hierarchy.
//!
//! The classes themselves live in `python/magik/exceptions.py`. This module
//! looks them up once, when the extension is imported, and then raises the right
//! one for each [`magik_core::ErrorKind`].
//!
//! ```text
//! MagikError                     (Exception)
//! |-- MagikOpenError
//! |-- MagikSaveError
//! |-- MagikOperationError
//! |-- MagikFormatError
//! |   `-- MagikUnsupportedFormatError
//! `-- MagikInternalError
//! ```
//!
//! # Diagnostics on every raise
//!
//! Each instance gets two attributes set before it is raised:
//!
//! * `kind` — a short stable string (`"open"`, `"save"`, ...);
//! * `imagemagick_detail` — the verbatim ImageMagick message, or `None`.
//!
//! The ImageMagick message is therefore both appended to the Python message and
//! preserved on the attribute: errors are translated, never swallowed.

use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyType;

use magik_core::{Error, ErrorKind};

/// The exception classes, resolved once from the Python package.
struct Exceptions {
    base: Py<PyType>,
    open: Py<PyType>,
    save: Py<PyType>,
    operation: Py<PyType>,
    format: Py<PyType>,
    unsupported_format: Py<PyType>,
    internal: Py<PyType>,
}

static EXCEPTIONS: PyOnceLock<Exceptions> = PyOnceLock::new();

/// Resolves the exception classes from `magik.exceptions`.
///
/// Must run during module initialisation, before any error can be raised. The
/// `magik` package imports `magik.exceptions` before it imports this extension,
/// so the module is already present in `sys.modules` here and importing it
/// cannot recurse.
pub fn register() -> PyResult<()> {
    Python::attach(|py| {
        let lookup = |name: &str| -> PyResult<Py<PyType>> {
            let module = py.import("magik.exceptions")?;
            let cls = module.getattr(name)?;
            Ok(cls.cast_into::<PyType>()?.unbind())
        };

        let bundle = Exceptions {
            base: lookup("MagikError")?,
            open: lookup("MagikOpenError")?,
            save: lookup("MagikSaveError")?,
            operation: lookup("MagikOperationError")?,
            format: lookup("MagikFormatError")?,
            unsupported_format: lookup("MagikUnsupportedFormatError")?,
            internal: lookup("MagikInternalError")?,
        };

        EXCEPTIONS
            .set(py, bundle)
            .map_err(|_| PyRuntimeError::new_err("magik exceptions are already registered"))
    })
}

/// Converts a [`magik_core::Error`] into the matching Python exception.
pub fn to_py_error(err: Error) -> PyErr {
    let kind = err.kind();
    let message = err.message();
    let detail = err.detail().map(str::to_string);

    Python::attach(|py| {
        let Some(exceptions) = EXCEPTIONS.get(py) else {
            // Only reachable if an error is somehow raised before the module
            // finished initialising. Report it rather than losing it.
            return PyErr::new::<PyRuntimeError, _>(message);
        };

        let cls = match kind {
            ErrorKind::Open => &exceptions.open,
            ErrorKind::Save => &exceptions.save,
            ErrorKind::Operation => &exceptions.operation,
            ErrorKind::Format => &exceptions.format,
            ErrorKind::UnsupportedFormat => &exceptions.unsupported_format,
            ErrorKind::Internal => &exceptions.internal,
        };

        build(py, cls, kind, &message, detail.as_deref())
    })
}

/// Instantiates `cls` and attaches the diagnostic attributes.
fn build(
    py: Python<'_>,
    cls: &Py<PyType>,
    kind: ErrorKind,
    message: &str,
    detail: Option<&str>,
) -> PyErr {
    let instance = match cls.bind(py).call1((message,)) {
        Ok(instance) => instance,
        Err(_) => return PyErr::new::<PyRuntimeError, _>(message.to_string()),
    };

    // The Python classes define both attributes at class level, so this only
    // adds an instance-level override.
    let _ = instance.setattr("kind", kind.as_str());
    let _ = instance.setattr("imagemagick_detail", detail);

    PyErr::from_value(instance)
}

/// The error raised when a Rust panic is caught at the Python boundary.
///
/// Reaching this means a bug in magik rather than a problem with the caller's
/// input, so it is deliberately loud and always a `MagikInternalError`.
pub(crate) fn internal_panic() -> PyErr {
    let err = magik_core::Error::internal(
        "internal error: a Rust panic was contained at the Python boundary",
    );
    to_py_error(err)
}

/// Wraps a `PyResult` and converts any [`magik_core::Error`] it contains.
pub trait IntoPyResult<T> {
    /// Converts `Result<T, magik_core::Error>` into `PyResult<T>`.
    fn py(self) -> PyResult<T>;
}

impl<T> IntoPyResult<T> for magik_core::Result<T> {
    fn py(self) -> PyResult<T> {
        self.map_err(to_py_error)
    }
}

/// Converts an argument that is neither a path nor a buffer into a `TypeError`.
///
/// Used by the `open()` / `save()` input coercion, where the user-facing failure
/// is a Python type problem rather than an ImageMagick problem.
pub fn type_error(message: impl std::fmt::Display) -> PyErr {
    PyTypeError::new_err(message.to_string())
}

/// The base exception class, exposed on the extension module for completeness.
pub fn base_class() -> PyResult<Py<PyType>> {
    Python::attach(|py| match EXCEPTIONS.get(py) {
        Some(exceptions) => Ok(exceptions.base.clone_ref(py)),
        None => Err(PyRuntimeError::new_err(
            "magik exceptions are not registered",
        )),
    })
}
