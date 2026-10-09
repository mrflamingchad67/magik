//! Raw FFI bindings to the ImageMagick 7 / MagickWand C API.
//!
//! # Scope
//!
//! This crate is the **only** place in the workspace where `unsafe` appears.
//! It contains:
//!
//!   * opaque C handle types ([`MagickWand`], [`PixelWand`]);
//!   * `extern "C"` declarations for the MagickWand entry points magik uses;
//!   * C enumeration aliases with their ImageMagick-defined discriminants;
//!   * `MagickBooleanType` and friends.
//!
//! It deliberately contains **no** ownership logic, no error handling, no
//! string decoding and no image abstractions — those belong to `magik-core`.
//!
//! # Safety contract
//!
//! Every function here is `unsafe` and carries the usual C obligations. The
//! safe wrappers in `magik-core` are the only sanctioned callers.
//!
//! # Stability
//!
//! These declarations were transcribed from the headers of an
//! ImageMagick 7.1.x `Q16-HDRI` build. Enum discriminants are part of the ABI,
//! so they are pinned to the values ImageMagick defines. Because they are
//! exposed as `c_uint` aliases rather than Rust `enum`s, an unknown value from
//! a newer ImageMagick can never produce undefined behaviour — it simply is not
//! equal to any known constant.
//!
//! No function in this crate spawns a process: all work happens in-process
//! through the MagickWand API.

#![allow(non_camel_case_types)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod wand;

pub use wand::{version, MagickVersion, PixelColor, Result, Wand, WandError};

use std::os::raw::{c_char, c_double, c_uchar, c_uint, c_void};

// ---------------------------------------------------------------------------
// Opaque handles
// ---------------------------------------------------------------------------

/// An opaque ImageMagick image wand.
///
/// Owned and managed exclusively by `magik-core`'s `Wand` wrapper. This crate
/// never allocates or frees one on its own.
#[repr(C)]
pub struct MagickWand {
    _private: [u8; 0],
}

/// An opaque ImageMagick pixel wand (a single colour value).
#[repr(C)]
pub struct PixelWand {
    _private: [u8; 0],
}

/// Return type of the ImageMagick C API.
///
/// ImageMagick spells this `MagickBooleanType`; it is a C enum and therefore
/// `int`-sized on every platform ImageMagick supports.
pub type MagickBooleanType = c_uint;

pub const MAGICK_FALSE: MagickBooleanType = 0;
pub const MAGICK_TRUE: MagickBooleanType = 1;

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// `ExceptionType` — the severity/class of an ImageMagick exception.
///
/// The constants below are ImageMagick's, whose numbering is *not* a plain
/// sequence: within a severity band each specific error shares the value of
/// its band's base.
pub type ExceptionType = c_uint;

pub const UNDEFINED_EXCEPTION: ExceptionType = 0;
pub const WARNING_EXCEPTION: ExceptionType = 300;
pub const RESOURCE_LIMIT_WARNING: ExceptionType = 300;
pub const TYPE_WARNING: ExceptionType = 305;
pub const OPTION_WARNING: ExceptionType = 310;
pub const DELEGATE_WARNING: ExceptionType = 315;
pub const MISSING_DELEGATE_WARNING: ExceptionType = 320;
pub const CORRUPT_IMAGE_WARNING: ExceptionType = 325;
pub const FILE_OPEN_WARNING: ExceptionType = 330;
pub const BLOB_WARNING: ExceptionType = 335;
pub const STREAM_WARNING: ExceptionType = 340;
pub const CACHE_WARNING: ExceptionType = 345;
pub const CODER_WARNING: ExceptionType = 350;
pub const FILTER_WARNING: ExceptionType = 352;
pub const MODULE_WARNING: ExceptionType = 355;
pub const DRAW_WARNING: ExceptionType = 360;
pub const IMAGE_WARNING: ExceptionType = 365;
pub const WAND_WARNING: ExceptionType = 370;
pub const RANDOM_WARNING: ExceptionType = 375;
pub const XSERVER_WARNING: ExceptionType = 380;
pub const MONITOR_WARNING: ExceptionType = 385;
pub const REGISTRY_WARNING: ExceptionType = 390;
pub const CONFIGURE_WARNING: ExceptionType = 395;
pub const POLICY_WARNING: ExceptionType = 399;

pub const ERROR_EXCEPTION: ExceptionType = 400;
pub const RESOURCE_LIMIT_ERROR: ExceptionType = 400;
pub const TYPE_ERROR: ExceptionType = 405;
pub const OPTION_ERROR: ExceptionType = 410;
pub const DELEGATE_ERROR: ExceptionType = 415;
pub const MISSING_DELEGATE_ERROR: ExceptionType = 420;
pub const CORRUPT_IMAGE_ERROR: ExceptionType = 425;
pub const FILE_OPEN_ERROR: ExceptionType = 430;
pub const BLOB_ERROR: ExceptionType = 435;
pub const STREAM_ERROR: ExceptionType = 440;
pub const CACHE_ERROR: ExceptionType = 445;
pub const CODER_ERROR: ExceptionType = 450;
pub const FILTER_ERROR: ExceptionType = 452;
pub const MODULE_ERROR: ExceptionType = 455;
pub const DRAW_ERROR: ExceptionType = 460;
pub const IMAGE_ERROR: ExceptionType = 465;
pub const WAND_ERROR: ExceptionType = 470;
pub const RANDOM_ERROR: ExceptionType = 475;
pub const XSERVER_ERROR: ExceptionType = 480;
pub const MONITOR_ERROR: ExceptionType = 480;
pub const REGISTRY_ERROR: ExceptionType = 490;
pub const CONFIGURE_ERROR: ExceptionType = 495;
pub const POLICY_ERROR: ExceptionType = 499;

pub const FATAL_ERROR_EXCEPTION: ExceptionType = 700;
pub const RESOURCE_LIMIT_FATAL_ERROR: ExceptionType = 700;
pub const TYPE_FATAL_ERROR: ExceptionType = 705;
pub const OPTION_FATAL_ERROR: ExceptionType = 710;
pub const DELEGATE_FATAL_ERROR: ExceptionType = 715;
pub const MISSING_DELEGATE_FATAL_ERROR: ExceptionType = 720;
pub const CORRUPT_IMAGE_FATAL_ERROR: ExceptionType = 725;
pub const FILE_OPEN_FATAL_ERROR: ExceptionType = 730;
pub const BLOB_FATAL_ERROR: ExceptionType = 735;
pub const STREAM_FATAL_ERROR: ExceptionType = 740;
pub const CACHE_FATAL_ERROR: ExceptionType = 745;
pub const CODER_FATAL_ERROR: ExceptionType = 750;
pub const FILTER_FATAL_ERROR: ExceptionType = 752;
pub const MODULE_FATAL_ERROR: ExceptionType = 755;
pub const DRAW_FATAL_ERROR: ExceptionType = 760;
pub const IMAGE_FATAL_ERROR: ExceptionType = 765;
pub const WAND_FATAL_ERROR: ExceptionType = 770;
pub const RANDOM_FATAL_ERROR: ExceptionType = 775;
pub const XSERVER_FATAL_ERROR: ExceptionType = 780;
pub const MONITOR_FATAL_ERROR: ExceptionType = 780;
pub const REGISTRY_FATAL_ERROR: ExceptionType = 790;
pub const CONFIGURE_FATAL_ERROR: ExceptionType = 795;
pub const POLICY_FATAL_ERROR: ExceptionType = 799;

/// `ImageType` — ImageMagick's structural classification of an image.
///
/// This is the authoritative signal magik uses to describe an image, because
/// MagickWand 7.x exposes no "how many channels" getter.
pub type ImageType = c_uint;

pub const UNDEFINED_TYPE: ImageType = 0;
pub const BILEVEL_TYPE: ImageType = 1;
pub const GRAYSCALE_TYPE: ImageType = 2;
pub const GRAYSCALE_ALPHA_TYPE: ImageType = 3;
pub const PALETTE_TYPE: ImageType = 4;
pub const PALETTE_ALPHA_TYPE: ImageType = 5;
pub const TRUECOLOR_TYPE: ImageType = 6;
pub const TRUECOLOR_ALPHA_TYPE: ImageType = 7;
pub const COLOR_SEPARATION_TYPE: ImageType = 8;
pub const COLOR_SEPARATION_ALPHA_TYPE: ImageType = 9;
pub const OPTIMIZE_TYPE: ImageType = 10;
pub const PALETTE_BILEVEL_ALPHA_TYPE: ImageType = 11;

/// `ColorspaceType` — a colour space as defined by ImageMagick.
pub type ColorspaceType = c_uint;

pub const UNDEFINED_COLORSPACE: ColorspaceType = 0;
pub const CMY_COLORSPACE: ColorspaceType = 1;
pub const CMYK_COLORSPACE: ColorspaceType = 2;
pub const GRAY_COLORSPACE: ColorspaceType = 3;
pub const HCL_COLORSPACE: ColorspaceType = 4;
pub const HCLP_COLORSPACE: ColorspaceType = 5;
pub const HSB_COLORSPACE: ColorspaceType = 6;
pub const HSI_COLORSPACE: ColorspaceType = 7;
pub const HSL_COLORSPACE: ColorspaceType = 8;
pub const HSV_COLORSPACE: ColorspaceType = 9;
pub const HWB_COLORSPACE: ColorspaceType = 10;
pub const LAB_COLORSPACE: ColorspaceType = 11;
pub const LCH_COLORSPACE: ColorspaceType = 12;
pub const LCHAB_COLORSPACE: ColorspaceType = 13;
pub const LCHUV_COLORSPACE: ColorspaceType = 14;
pub const LOG_COLORSPACE: ColorspaceType = 15;
pub const LMS_COLORSPACE: ColorspaceType = 16;
pub const LUV_COLORSPACE: ColorspaceType = 17;
pub const OHTA_COLORSPACE: ColorspaceType = 18;
pub const REC601YCB_CR_COLORSPACE: ColorspaceType = 19;
pub const REC709YCB_CR_COLORSPACE: ColorspaceType = 20;
pub const RGB_COLORSPACE: ColorspaceType = 21;
pub const SCRGB_COLORSPACE: ColorspaceType = 22;
pub const SRGB_COLORSPACE: ColorspaceType = 23;
pub const TRANSPARENT_COLORSPACE: ColorspaceType = 24;
pub const XYY_COLORSPACE: ColorspaceType = 25;
pub const XYZ_COLORSPACE: ColorspaceType = 26;
pub const YCBCR_COLORSPACE: ColorspaceType = 27;
pub const YCC_COLORSPACE: ColorspaceType = 28;
pub const YDBDR_COLORSPACE: ColorspaceType = 29;
pub const YIQ_COLORSPACE: ColorspaceType = 30;
pub const YPBPR_COLORSPACE: ColorspaceType = 31;
pub const YUV_COLORSPACE: ColorspaceType = 32;
pub const LINEAR_GRAY_COLORSPACE: ColorspaceType = 33;
pub const JZAZBZ_COLORSPACE: ColorspaceType = 34;
pub const DISPLAY_P3_COLORSPACE: ColorspaceType = 35;
pub const ADOBE98_COLORSPACE: ColorspaceType = 36;
pub const PROPHOTO_COLORSPACE: ColorspaceType = 37;
pub const OKLAB_COLORSPACE: ColorspaceType = 38;
pub const OKLCH_COLORSPACE: ColorspaceType = 39;

/// `CompressionType` — the compression scheme used by a codec.
pub type CompressionType = c_uint;

pub const UNDEFINED_COMPRESSION: CompressionType = 0;
pub const B44A_COMPRESSION: CompressionType = 1;
pub const B44_COMPRESSION: CompressionType = 2;
pub const BZIP_COMPRESSION: CompressionType = 3;
pub const DXT1_COMPRESSION: CompressionType = 4;
pub const DXT3_COMPRESSION: CompressionType = 5;
pub const DXT5_COMPRESSION: CompressionType = 6;
pub const FAX_COMPRESSION: CompressionType = 7;
pub const GROUP4_COMPRESSION: CompressionType = 8;
pub const JBIG1_COMPRESSION: CompressionType = 9;
pub const JBIG2_COMPRESSION: CompressionType = 10;
pub const JPEG2000_COMPRESSION: CompressionType = 11;
pub const JPEG_COMPRESSION: CompressionType = 12;
pub const LOSSLESS_JPEG_COMPRESSION: CompressionType = 13;
pub const LZMA_COMPRESSION: CompressionType = 14;
pub const LZW_COMPRESSION: CompressionType = 15;
pub const NO_COMPRESSION: CompressionType = 16;
pub const PIZ_COMPRESSION: CompressionType = 17;
pub const PXR24_COMPRESSION: CompressionType = 18;
pub const RLE_COMPRESSION: CompressionType = 19;
pub const ZIP_COMPRESSION: CompressionType = 20;
pub const ZIPS_COMPRESSION: CompressionType = 21;
pub const ZSTD_COMPRESSION: CompressionType = 22;
pub const WEBP_COMPRESSION: CompressionType = 23;
pub const DWAA_COMPRESSION: CompressionType = 24;
pub const DWAB_COMPRESSION: CompressionType = 25;
pub const BC7_COMPRESSION: CompressionType = 26;
pub const BC5_COMPRESSION: CompressionType = 27;
pub const LERC_COMPRESSION: CompressionType = 28;

/// `FilterType` — resampling kernels, used by `MagickResizeImage`.
pub type FilterType = c_uint;

pub const UNDEFINED_FILTER: FilterType = 0;
pub const POINT_FILTER: FilterType = 1;
pub const BOX_FILTER: FilterType = 2;
pub const TRIANGLE_FILTER: FilterType = 3;
pub const HERMITE_FILTER: FilterType = 4;
pub const HANN_FILTER: FilterType = 5;
pub const HAMMING_FILTER: FilterType = 6;
pub const BLACKMAN_FILTER: FilterType = 7;
pub const GAUSSIAN_FILTER: FilterType = 8;
pub const QUADRATIC_FILTER: FilterType = 9;
pub const CUBIC_FILTER: FilterType = 10;
pub const CATROM_FILTER: FilterType = 11;
pub const MITCHELL_FILTER: FilterType = 12;
pub const JINC_FILTER: FilterType = 13;
pub const SINC_FILTER: FilterType = 14;
pub const SINC_FAST_FILTER: FilterType = 15;
pub const KAISER_FILTER: FilterType = 16;
pub const WELCH_FILTER: FilterType = 17;
pub const PARZEN_FILTER: FilterType = 18;
pub const BOHMAN_FILTER: FilterType = 19;
pub const BARTLETT_FILTER: FilterType = 20;
pub const LAGRANGE_FILTER: FilterType = 21;
pub const LANCZOS_FILTER: FilterType = 22;
pub const LANCZOS_SHARP_FILTER: FilterType = 23;
pub const LANCZOS2_FILTER: FilterType = 24;
pub const LANCZOS2_SHARP_FILTER: FilterType = 25;
pub const ROBIDOUX_FILTER: FilterType = 26;
pub const ROBIDOUX_SHARP_FILTER: FilterType = 27;
pub const COSINE_FILTER: FilterType = 28;
pub const SPLINE_FILTER: FilterType = 29;
pub const LANCZOS_RADIUS_FILTER: FilterType = 30;
pub const CUBIC_SPLINE_FILTER: FilterType = 31;
pub const MAGIC_KERNEL_SHARP_2013_FILTER: FilterType = 32;
pub const MAGIC_KERNEL_SHARP_2021_FILTER: FilterType = 33;
pub const SENTINEL_FILTER: FilterType = 34;

// ---------------------------------------------------------------------------
// Library lifecycle
// ---------------------------------------------------------------------------

// Initialise the ImageMagick environment.
//
// ImageMagick documents this as idempotent and thread-safe; `magik-core` calls
// it exactly once via a process-wide guard.
extern "C" {
    pub fn MagickWandGenesis();
}

// Release resources held by the ImageMagick environment.
extern "C" {
    pub fn MagickWandTerminus();
}

// ---------------------------------------------------------------------------
// Wand lifecycle
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    /// Allocate and initialise an empty wand.
    ///
    /// # Safety
    /// The returned pointer is owned by the caller and must be released with
    /// [`DestroyMagickWand`] exactly once.
    pub fn NewMagickWand() -> *mut MagickWand;

    /// Release a wand, returning `NULL`.
    ///
    /// # Safety
    /// `wand` must have come from [`NewMagickWand`] or [`CloneMagickWand`] and
    /// must not be used afterwards. Passing `NULL` is a no-op.
    pub fn DestroyMagickWand(wand: *mut MagickWand) -> *mut MagickWand;

    /// Deep-copy a wand, including its image list.
    ///
    /// # Safety
    /// `wand` must be a valid, live wand. The result must be released with
    /// [`DestroyMagickWand`].
    pub fn CloneMagickWand(wand: *const MagickWand) -> *mut MagickWand;

    /// Release the wand's images while keeping the wand usable.
    ///
    /// # Safety
    /// `wand` must be a valid, live wand.
    pub fn ClearMagickWand(wand: *mut MagickWand) -> MagickBooleanType;

    /// Fetch the pending exception message and severity from a wand.
    ///
    /// The returned string is heap-allocated by ImageMagick and must be freed
    /// with [`MagickRelinquishMemory`].
    ///
    /// # Safety
    /// `wand` must be a valid, live wand. `severity` must be a valid pointer.
    pub fn MagickGetException(wand: *const MagickWand, severity: *mut ExceptionType)
        -> *mut c_char;

    /// Free a buffer previously allocated by ImageMagick.
    ///
    /// # Safety
    /// `ptr` must have come from ImageMagick (e.g. [`MagickGetException`] or
    /// [`MagickGetImageBlob`]) and must not be used afterwards.
    pub fn MagickRelinquishMemory(ptr: *mut c_void) -> *mut c_void;
}

// ---------------------------------------------------------------------------
// Reading and writing images
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    /// Load an image from a filesystem path.
    ///
    /// # Safety
    /// `wand` must be valid; `path` must be a NUL-terminated C string.
    pub fn MagickReadImage(wand: *mut MagickWand, path: *const c_char) -> MagickBooleanType;

    /// Load an image from an in-memory blob.
    ///
    /// # Safety
    /// `wand` must be valid; `blob` must point to `length` readable bytes.
    pub fn MagickReadImageBlob(
        wand: *mut MagickWand,
        blob: *const c_void,
        length: usize,
    ) -> MagickBooleanType;

    /// Write the wand's images to `path`.
    ///
    /// The output format is taken from [`MagickSetImageFormat`] when set,
    /// otherwise it is inferred from the filename extension.
    ///
    /// # Safety
    /// `wand` must be valid; `path` must be a NUL-terminated C string.
    pub fn MagickWriteImage(wand: *mut MagickWand, path: *const c_char) -> MagickBooleanType;

    /// Encode the wand's images into a newly allocated blob.
    ///
    /// The buffer must be freed with [`MagickRelinquishMemory`].
    ///
    /// # Safety
    /// `wand` must be valid; `length` must be a valid pointer.
    pub fn MagickGetImageBlob(wand: *mut MagickWand, length: *mut usize) -> *mut c_uchar;
}

// ---------------------------------------------------------------------------
// Image properties
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    pub fn MagickGetImageWidth(wand: *mut MagickWand) -> usize;
    pub fn MagickGetImageHeight(wand: *mut MagickWand) -> usize;

    /// Bits of precision per colour channel (not bits per pixel).
    pub fn MagickGetImageDepth(wand: *mut MagickWand) -> usize;

    pub fn MagickGetImageColorspace(wand: *mut MagickWand) -> ColorspaceType;
    pub fn MagickSetImageColorspace(
        wand: *mut MagickWand,
        colorspace: ColorspaceType,
    ) -> MagickBooleanType;

    pub fn MagickGetImageCompression(wand: *mut MagickWand) -> CompressionType;
    pub fn MagickSetImageCompression(
        wand: *mut MagickWand,
        compression: CompressionType,
    ) -> MagickBooleanType;

    pub fn MagickGetImageCompressionQuality(wand: *mut MagickWand) -> usize;
    pub fn MagickSetImageCompressionQuality(
        wand: *mut MagickWand,
        quality: usize,
    ) -> MagickBooleanType;

    pub fn MagickGetImageType(wand: *mut MagickWand) -> ImageType;

    /// The image's magick format (e.g. `"PNG"`).
    ///
    /// The returned string is heap-allocated and must be freed with
    /// [`MagickRelinquishMemory`].
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickGetImageFormat(wand: *mut MagickWand) -> *mut c_char;

    /// Select the encoder used when the image is written.
    ///
    /// # Safety
    /// `wand` must be valid; `format` must be a NUL-terminated C string.
    pub fn MagickSetImageFormat(wand: *mut MagickWand, format: *const c_char) -> MagickBooleanType;

    /// Read a wand/format-level option (e.g. `"png:color-type"`).
    ///
    /// The returned string is heap-allocated; free it with
    /// [`MagickRelinquishMemory`]. `NULL` means the option is unset.
    ///
    /// # Safety
    /// `wand` must be valid; `key` must be a NUL-terminated C string.
    pub fn MagickGetOption(wand: *mut MagickWand, key: *const c_char) -> *mut c_char;

    /// Set a wand/format-level option.
    ///
    /// # Safety
    /// `wand` must be valid; `key` and `value` must be NUL-terminated C strings.
    pub fn MagickSetOption(
        wand: *mut MagickWand,
        key: *const c_char,
        value: *const c_char,
    ) -> MagickBooleanType;
}

// ---------------------------------------------------------------------------
// ImageMagick build information
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    /// The linked ImageMagick version string (e.g. `"7.1.2-32 Q16 HDRI x64 ..."`).
    ///
    /// Returns a pointer to a static string that must **not** be freed.
    ///
    /// # Safety
    /// `length` must be a valid pointer.
    pub fn MagickGetVersion(length: *mut usize) -> *const c_char;

    /// Quantum depth description (e.g. `"Q16"`). Static string; do not free.
    ///
    /// # Safety
    /// `length` must be a valid pointer.
    pub fn MagickGetQuantumDepth(length: *mut usize) -> *const c_char;

    /// Quantum range description (e.g. `"QuantumRange.Max = 65535"`). Static
    /// string; do not free.
    ///
    /// # Safety
    /// `length` must be a valid pointer.
    pub fn MagickGetQuantumRange(length: *mut usize) -> *const c_char;
}

// ---------------------------------------------------------------------------
// Transformations
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    /// Resample the image to `columns` x `rows` using `filter`.
    ///
    /// Note the ImageMagick 7 signature: unlike ImageMagick 6 there is no
    /// `blur` parameter — blur is a separate operation ([`MagickBlurImage`]).
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickResizeImage(
        wand: *mut MagickWand,
        columns: usize,
        rows: usize,
        filter: FilterType,
    ) -> MagickBooleanType;

    /// Extract a `width` x `height` region whose top-left corner is `(x, y)`.
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickCropImage(
        wand: *mut MagickWand,
        width: usize,
        height: usize,
        x: isize,
        y: isize,
    ) -> MagickBooleanType;

    /// Rotate the image by `degrees` about the centre, filling any exposed
    /// area with `background`.
    ///
    /// # Safety
    /// `wand` must be valid; `background` must be a live pixel wand or `NULL`.
    pub fn MagickRotateImage(
        wand: *mut MagickWand,
        background: *const PixelWand,
        degrees: c_double,
    ) -> MagickBooleanType;

    /// Mirror the image vertically (top becomes bottom).
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickFlipImage(wand: *mut MagickWand) -> MagickBooleanType;

    /// Mirror the image horizontally (left becomes right).
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickFlopImage(wand: *mut MagickWand) -> MagickBooleanType;

    /// Apply a Gaussian blur.
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickBlurImage(
        wand: *mut MagickWand,
        radius: c_double,
        sigma: c_double,
    ) -> MagickBooleanType;

    /// Convert the image into `colorspace`.
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickTransformImageColorspace(
        wand: *mut MagickWand,
        colorspace: ColorspaceType,
    ) -> MagickBooleanType;
}

// ---------------------------------------------------------------------------
// Pixel wands (single colours)
// ---------------------------------------------------------------------------

// Link directives for `CORE_RL_MagickWand_` are emitted by `build.rs`, which is
// the single source of truth for how the native library is located.
extern "C" {
    /// Allocate an empty pixel wand.
    ///
    /// # Safety
    /// The result must be released with [`DestroyPixelWand`].
    pub fn NewPixelWand() -> *mut PixelWand;

    /// Release a pixel wand, returning `NULL`.
    ///
    /// # Safety
    /// `pw` must have come from [`NewPixelWand`] and must not be used
    /// afterwards. Passing `NULL` is a no-op.
    pub fn DestroyPixelWand(pw: *mut PixelWand) -> *mut PixelWand;

    /// Set a pixel wand's colour from an ImageMagick colour specification.
    ///
    /// # Safety
    /// `pw` must be valid; `color` must be a NUL-terminated C string.
    pub fn PixelSetColor(pw: *mut PixelWand, color: *const c_char) -> MagickBooleanType;

    /// Render a pixel wand as an ImageMagick colour string (e.g. `"#ff0000"`).
    ///
    /// The returned string is heap-allocated and must be freed with
    /// [`MagickRelinquishMemory`].
    ///
    /// # Safety
    /// `pw` must be valid.
    pub fn PixelGetColorAsString(pw: *const PixelWand) -> *mut c_char;
}

// ---------------------------------------------------------------------------
// Bulk and per-pixel pixel access
//
// These are the entry points magik uses for pixel-level work. They are
// deliberately preferred over reading a pixel wand's `Quantum` components
// directly: see the note on `StorageType` below for why.
// ---------------------------------------------------------------------------

/// `StorageType` — the sample layout expected by the pixel import/export calls.
///
/// Magik-core always selects [`CHAR_PIXEL`] (8-bit unsigned) or [`SHORT_PIXEL`]
/// (16-bit unsigned) and never `QuantumPixel`.
///
/// That is deliberate. `Quantum` is `float` on HDRI builds and `unsigned short`
/// on non-HDRI Q16 builds, so its width cannot be known at compile time without
/// parsing `magick-baseconfig.h`. Picking an explicit integer sample type lets
/// ImageMagick perform the HDRI -> integer scaling itself, so the Rust side only
/// ever handles `u8` and `u16` and the result does not depend on the build's
/// quantum configuration.
pub type StorageType = c_uint;

pub const UNDEFINED_PIXEL: StorageType = 0;
pub const CHAR_PIXEL: StorageType = 1;
pub const DOUBLE_PIXEL: StorageType = 2;
pub const FLOAT_PIXEL: StorageType = 3;
pub const LONG_PIXEL: StorageType = 4;
pub const LONG_LONG_PIXEL: StorageType = 5;
pub const QUANTUM_PIXEL: StorageType = 6;
pub const SHORT_PIXEL: StorageType = 7;

/// `AlphaChannelOption` — how an alpha channel is activated, removed or shaped.
pub type AlphaChannelOption = c_uint;

pub const UNDEFINED_ALPHA_CHANNEL: AlphaChannelOption = 0;
pub const ACTIVATE_ALPHA_CHANNEL: AlphaChannelOption = 1;
pub const ASSOCIATE_ALPHA_CHANNEL: AlphaChannelOption = 2;
pub const BACKGROUND_ALPHA_CHANNEL: AlphaChannelOption = 3;
pub const COPY_ALPHA_CHANNEL: AlphaChannelOption = 4;
pub const DEACTIVATE_ALPHA_CHANNEL: AlphaChannelOption = 5;
pub const DISCRETE_ALPHA_CHANNEL: AlphaChannelOption = 6;
pub const DISASSOCIATE_ALPHA_CHANNEL: AlphaChannelOption = 7;
pub const EXTRACT_ALPHA_CHANNEL: AlphaChannelOption = 8;
pub const OFF_ALPHA_CHANNEL: AlphaChannelOption = 9;
pub const ON_ALPHA_CHANNEL: AlphaChannelOption = 10;
pub const OPAQUE_ALPHA_CHANNEL: AlphaChannelOption = 11;
pub const REMOVE_ALPHA_CHANNEL: AlphaChannelOption = 12;
pub const SET_ALPHA_CHANNEL: AlphaChannelOption = 13;
pub const SHAPE_ALPHA_CHANNEL: AlphaChannelOption = 14;
pub const TRANSPARENT_ALPHA_CHANNEL: AlphaChannelOption = 15;
pub const OFF_IF_OPAQUE_ALPHA_CHANNEL: AlphaChannelOption = 16;

extern "C" {
    /// Copy a rectangular region of pixels out of the wand into `pixels`.
    ///
    /// `map` selects and orders the channels (for example `"RGB"`, `"RGBA"`,
    /// `"CMYK"`); passing `NULL` uses the image's own channel map. Samples are
    /// written interleaved, row by row, and `pixels` must therefore hold at
    /// least `columns * rows * samples_per_pixel * sizeof(storage)` bytes.
    ///
    /// Note that ImageMagick's `SetPixelChannelMap` — which interprets `map` —
    /// is an internal function with no header declaration, so the accepted
    /// characters are verified empirically by magik's tests rather than being
    /// transcribed here.
    ///
    /// # Safety
    /// `wand` must be valid; `map` must be a NUL-terminated string or `NULL`;
    /// `pixels` must be writable for the full extent implied by the arguments.
    pub fn MagickExportImagePixels(
        wand: *mut MagickWand,
        x: isize,
        y: isize,
        columns: usize,
        rows: usize,
        map: *const c_char,
        storage: StorageType,
        pixels: *mut c_void,
    ) -> MagickBooleanType;

    /// Replace a rectangular region of the wand with `pixels`.
    ///
    /// This **overwrites** the region; it does not blend. `map` and `storage`
    /// have the same meaning as in [`MagickExportImagePixels`].
    ///
    /// # Safety
    /// `wand` must be valid; `map` must be a NUL-terminated string or `NULL`;
    /// `pixels` must be readable for the full extent implied by the arguments.
    pub fn MagickImportImagePixels(
        wand: *mut MagickWand,
        x: isize,
        y: isize,
        columns: usize,
        rows: usize,
        map: *const c_char,
        storage: StorageType,
        pixels: *const c_void,
    ) -> MagickBooleanType;

    /// Read the colour of a single pixel at `(x, y)` into `pixel`.
    ///
    /// # Safety
    /// `wand` and `pixel` must both be valid.
    pub fn MagickGetImagePixelColor(
        wand: *mut MagickWand,
        x: isize,
        y: isize,
        pixel: *mut PixelWand,
    ) -> MagickBooleanType;

    /// Set the colour of a single pixel at `(x, y)`.
    ///
    /// # Safety
    /// `wand` must be valid and `pixel` must be a live pixel wand.
    pub fn MagickSetImagePixelColor(
        wand: *mut MagickWand,
        x: isize,
        y: isize,
        pixel: *const PixelWand,
    ) -> MagickBooleanType;
}

// ---------------------------------------------------------------------------
// Image construction
// ---------------------------------------------------------------------------

extern "C" {
    /// Create a `columns` x `rows` image filled with `background`.
    ///
    /// # Safety
    /// `wand` must be valid; `background` must be a live pixel wand or `NULL`
    /// (in which case the image starts black).
    pub fn MagickNewImage(
        wand: *mut MagickWand,
        columns: usize,
        rows: usize,
        background: *const PixelWand,
    ) -> MagickBooleanType;

    /// Force the image's structural classification.
    ///
    /// Useful for making a freshly filled image report the mode the caller asked
    /// for (for example `GRAYSCALE_TYPE`).
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickSetImageType(wand: *mut MagickWand, image_type: ImageType) -> MagickBooleanType;

    /// Activate, remove or otherwise transform the alpha channel.
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickSetImageAlphaChannel(
        wand: *mut MagickWand,
        alpha_channel: AlphaChannelOption,
    ) -> MagickBooleanType;

    /// Test whether the image has an alpha channel.
    ///
    /// # Safety
    /// `wand` must be valid.
    pub fn MagickGetImageAlphaChannel(wand: *mut MagickWand) -> MagickBooleanType;
}
