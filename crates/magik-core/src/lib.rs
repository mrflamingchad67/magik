//! `magik-core` — the safe, idiomatic Rust image abstraction over ImageMagick.
//!
//! ```no_run
//! use magik_core::Image;
//!
//! let image = Image::open("input.jpg")?;
//! let image = image.resize(800, 600)?;
//! image.grayscale()?.save("output.webp")?;
//! # Ok::<(), magik_core::Error>(())
//! ```
//!
//! # Layering
//!
//! ```text
//! magik-sys   raw FFI, all `unsafe`
//!     ^
//! magik-core  safe ownership + operations   <- this crate
//! ```
//!
//! `magik-core` depends on `magik-sys` and nothing else. It does **not** know
//! about Python or PyO3, and it never spawns a subprocess: every operation is an
//! in-process MagickWand call.
//!
//! # Unsafe policy
//!
//! All `unsafe` in this crate lives in [`magick`], in a handful of small blocks
//! that wrap individual FFI calls. [`image`] — where the actual image logic
//! lives — is written entirely in safe Rust.
//!
//! # Threading
//!
//! [`image::Image`] is [`Send`] **and** [`Sync`]. That is not an unchecked
//! promise: `Image` keeps its wand behind a `Mutex`, which is exactly the
//! serialisation ImageMagick requires, so two threads may call into the same
//! image safely. Copies (from [`image::Image::try_clone`] and from every
//! operation) own their own wand and mutex and can be used in parallel on
//! different threads.
//!
//! `Wand` itself is only [`Send`], never [`Sync`], so a raw wand can never be
//! shared by reference across threads. The `Image`-level `Send + Sync` is the
//! only supported way to move images between threads.

#![deny(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]

pub mod error;
pub mod image;
pub mod magick;
pub mod pixel;

pub use error::{Error, ErrorKind, Result};
pub use image::{filter_by_name, imagemagick_version, Image, ImageMetadata};
pub use magick::{
    available_channels, channel_from_name, channel_names, channels_for_image_type,
    colorspace_from_name, colorspace_name, compression_from_name, compression_name,
    filter_from_name, filter_name, image_type_name, MagickVersion, PixelColor, Wand,
    DEFAULT_FILTER,
};
pub use pixel::{PixelMode, SampleDepth};

/// Re-exported so downstream layers can reason about ImageMagick's ABI
/// constants without depending on `magik-sys` directly.
pub use magik_sys;

/// Low-level helpers that are useful to a Python-facing layer.
pub mod prelude {
    pub use crate::error::{Error, ErrorKind, Result};
    pub use crate::image::{filter_by_name, Image, ImageMetadata};
    pub use crate::magick::MagickVersion;
}
