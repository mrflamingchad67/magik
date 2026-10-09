//! Python-surface helpers for the pixel API.
//!
//! The heavy lifting lives in `magik-core`; this module only converts between
//! Python objects and the core types, and it contains no `unsafe` and no
//! ImageMagick knowledge.
//!
//! # Return-shape rules
//!
//! magik follows Pillow for single-pixel reads: a mode with one sample channel
//! yields a bare `int`, anything else yields a `tuple`. Bulk reads always yield
//! `bytes`, which keeps the Python boundary crossed exactly once per operation.

use pyo3::prelude::*;
use pyo3::types::PyTuple;

use magik_core::{PixelMode, SampleDepth};

use crate::exception::{to_py_error, type_error};

/// The pixel modes magik supports, in a form Python can introspect.
pub const MODES: &[&str] = &["1", "L", "P", "RGB", "RGBA", "CMYK"];

/// Extracts a bytes-like object into an owned buffer.
///
/// Accepts `bytes`, `bytearray`, `memoryview` and anything else exporting the
/// buffer protocol, matching what `Image.open` already accepts. The copy is
/// unavoidable: the bytes are handed to Rust as a slice and must outlive the
/// call.
pub fn coerce_buffer(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    match pyo3::buffer::PyBuffer::<u8>::get(obj) {
        Ok(buffer) => buffer.to_vec(obj.py()),
        Err(_) => Err(type_error(format!(
            "expected a bytes-like object, got {}",
            obj.get_type().name()?
        ))),
    }
}

/// Parses a `depth=` argument. `None` means 8 bits, the Pillow-like default.
pub fn parse_depth(depth: Option<u8>) -> PyResult<SampleDepth> {
    let bits = depth.unwrap_or(8);
    SampleDepth::from_bits(bits).map_err(to_py_error)
}

/// Parses a `(width, height)` argument.
pub fn coerce_size_2d(size: &Bound<'_, PyAny>) -> PyResult<(u32, u32)> {
    let parsed: (i64, i64) = size
        .extract()
        .map_err(|_| type_error("size must be a (width, height) pair of integers"))?;
    let to_u32 = |value: i64, name: &str| -> PyResult<u32> {
        u32::try_from(value).ok().filter(|v| *v > 0).ok_or_else(|| {
            to_py_error(magik_core::Error::operation(format!(
                "invalid {name} {value}: dimensions must be positive integers"
            )))
        })
    };
    Ok((to_u32(parsed.0, "width")?, to_u32(parsed.1, "height")?))
}

/// Parses an `(x, y)` argument.
pub fn coerce_xy(point: &Bound<'_, PyAny>) -> PyResult<(i64, i64)> {
    point
        .extract::<(i64, i64)>()
        .map_err(|_| type_error("coordinates must be an (x, y) pair of integers"))
}

/// Parses a pixel value into sample values matching `mode`.
///
/// A single `int` fills a single-sample mode; a sequence fills any mode and must
/// have exactly one entry per channel.
pub fn coerce_pixel_value(
    value: &Bound<'_, PyAny>,
    mode: PixelMode,
    depth: SampleDepth,
) -> PyResult<Vec<u32>> {
    let max = depth.max_value();

    // An integer is only meaningful for a single-channel mode.
    if let Ok(number) = value.extract::<i64>() {
        return if mode.is_scalar() {
            Ok(vec![clamp(number, max)])
        } else {
            Err(type_error(format!(
                "mode {:?} needs {} sample(s); pass a sequence of that length",
                mode.name(),
                mode.channels()
            )))
        };
    }

    let sequence: Vec<i64> = value
        .extract()
        .map_err(|_| type_error("pixel value must be an int or a sequence of ints"))?;

    if sequence.len() != mode.channels() {
        return Err(to_py_error(magik_core::Error::operation(format!(
            "mode {:?} needs {} sample(s) but got {}",
            mode.name(),
            mode.channels(),
            sequence.len()
        ))));
    }
    Ok(sequence
        .into_iter()
        .map(|number| clamp(number, max))
        .collect())
}

/// Clamps a Python integer into the valid sample range.
fn clamp(value: i64, max: u32) -> u32 {
    if value < 0 {
        0
    } else if value as u64 > u64::from(max) {
        max
    } else {
        value as u32
    }
}

/// Wraps sample values the way Pillow shapes `getpixel`.
///
/// A single-sample mode yields a bare `int`; every other mode yields a `tuple`.
/// This is Pillow's convention, and it is why `Image.getpixel` and
/// `Image.putpixel` accept both shapes symmetrically.
pub fn samples_to_object(mode: PixelMode, samples: &[u32]) -> Py<PyAny> {
    Python::attach(|py| {
        if mode.is_scalar() {
            let value = samples.first().copied().unwrap_or(0);
            return value
                .into_pyobject(py)
                .map(Into::into)
                .unwrap_or_else(|_| py.None());
        }
        PyTuple::new(py, samples.iter().copied())
            .map(Into::into)
            .unwrap_or_else(|_| py.None())
    })
}

/// The canonical colour for a mode, used as a default fill.
pub fn default_color(mode: PixelMode) -> &'static str {
    match mode {
        PixelMode::Rgba => "rgba(0,0,0,255)",
        PixelMode::Cmyk => "cmyk(65535,65535,65535,65535)",
        _ => "black",
    }
}
