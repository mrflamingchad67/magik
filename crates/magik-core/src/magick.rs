//! ImageMagick vocabulary and error policy — the purely-safe half of the core.
//!
//! # What this module is for
//!
//! The raw handles and every `unsafe` block live in `magik-sys::wand`. This
//! module adds the two things that are *policy* rather than FFI:
//!
//! 1. **Naming** — turning ImageMagick's numeric `ColorspaceType`,
//!    `CompressionType`, `ImageType` and `FilterType` values into stable
//!    strings, and resolving strings back to values.
//! 2. **Error classification** — turning a [`WandError`] into a
//!    [`magik_core::Error`] with the right [`ErrorKind`].
//!
//! It contains **no `unsafe`**, no raw pointers, and no ownership logic. The
//! `Wand` and `PixelColor` types are re-exported from `magik-sys` unchanged, so
//! callers get the safe wrapper without a second abstraction layer.
//!
//! # Threading
//!
//! A [`Wand`] may be *moved* between threads but must not be *used* from two
//! threads at once, so it is [`Send`] and deliberately not [`Sync`]. The
//! sound way to share one is [`crate::image::Image`], which keeps it behind a
//! mutex.

pub use magik_sys::wand::{version, MagickVersion, PixelColor, Wand, WandError};

use magik_sys as sys;

use crate::error::{Error, ErrorKind};

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Whether an ImageMagick severity indicates a missing or non-functional
/// delegate.
fn is_delegate_severity(severity: u32) -> bool {
    matches!(
        severity,
        sys::DELEGATE_ERROR
            | sys::MISSING_DELEGATE_ERROR
            | sys::DELEGATE_WARNING
            | sys::MISSING_DELEGATE_WARNING
            | sys::DELEGATE_FATAL_ERROR
            | sys::MISSING_DELEGATE_FATAL_ERROR
    )
}

/// Whether a failure means "this build cannot handle that format".
///
/// The severity codes alone are not enough: ImageMagick reports
/// `"no encode delegate for this image format `XCF'"` from its write path with a
/// severity that is *not* one of the delegate codes, so the message text is
/// checked too. That makes such a failure surface as
/// [`ErrorKind::UnsupportedFormat`] rather than a generic save failure.
///
/// This is a heuristic over ImageMagick's wording. If a future release rephrases
/// it, the worst case is that such a failure is reported one level more
/// generically — never silently swallowed.
fn is_delegate_failure(error: &WandError) -> bool {
    if is_delegate_severity(error.severity) {
        return true;
    }
    let Some(message) = error.message.as_deref() else {
        return false;
    };
    let lower = message.to_ascii_lowercase();
    lower.contains("no encode delegate")
        || lower.contains("no decode delegate")
        || lower.contains("no delegate for this image format")
}

/// Builds a core [`Error`] from a raw [`WandError`], applying the delegate rule.
pub(crate) fn classify(kind: ErrorKind, context: impl Into<String>, error: WandError) -> Error {
    let context = context.into();
    let detail = error.message.clone();
    if is_delegate_failure(&error) && matches!(kind, ErrorKind::Save | ErrorKind::Format) {
        return Error::unsupported_format(context).with_optional_detail(detail);
    }
    Error::new(kind, context).with_optional_detail(detail)
}

/// The number of colour channels implied by an `ImageType`.
///
/// ImageMagick 7.1's MagickWand API exposes **no** "how many channels does this
/// image have" getter, so magik derives the count from the image's structural
/// classification — the same information ImageMagick itself uses to decide how
/// many samples each pixel holds.
pub fn channels_for_image_type(value: u32) -> u32 {
    match value {
        // Grey, bilevel and palette images store a single colour index/sample.
        sys::BILEVEL_TYPE | sys::GRAYSCALE_TYPE | sys::PALETTE_TYPE => 1,
        sys::GRAYSCALE_ALPHA_TYPE | sys::PALETTE_ALPHA_TYPE | sys::PALETTE_BILEVEL_ALPHA_TYPE => 2,
        // A separation image is CMYK: C, M, Y and K.
        sys::COLOR_SEPARATION_TYPE => 4,
        sys::COLOR_SEPARATION_ALPHA_TYPE => 5,
        // Optimize is ImageMagick's internal state and behaves as RGB.
        sys::TRUECOLOR_TYPE | sys::OPTIMIZE_TYPE => 3,
        sys::TRUECOLOR_ALPHA_TYPE => 4,
        // Undefined/Unknown: report 0 rather than guess.
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Name <-> value mapping
//
// MagickWand 7.x ships no name parser (the `ParseCommandOption` entry point
// drags in two further large enums), so magik carries small explicit tables.
// They are additive: extending them is how later stages grow coverage without
// touching the architecture.
// ---------------------------------------------------------------------------

/// Canonical ImageMagick name for a `ColorspaceType` value.
pub fn colorspace_name(value: u32) -> &'static str {
    match value {
        sys::UNDEFINED_COLORSPACE => "Undefined",
        sys::CMY_COLORSPACE => "CMY",
        sys::CMYK_COLORSPACE => "CMYK",
        sys::GRAY_COLORSPACE => "Gray",
        sys::HCL_COLORSPACE => "HCL",
        sys::HCLP_COLORSPACE => "HCLp",
        sys::HSB_COLORSPACE => "HSB",
        sys::HSI_COLORSPACE => "HSI",
        sys::HSL_COLORSPACE => "HSL",
        sys::HSV_COLORSPACE => "HSV",
        sys::HWB_COLORSPACE => "HWB",
        sys::LAB_COLORSPACE => "Lab",
        sys::LCH_COLORSPACE => "LCH",
        sys::LCHAB_COLORSPACE => "LCHab",
        sys::LCHUV_COLORSPACE => "LCHuv",
        sys::LOG_COLORSPACE => "Log",
        sys::LMS_COLORSPACE => "LMS",
        sys::LUV_COLORSPACE => "Luv",
        sys::OHTA_COLORSPACE => "OHTA",
        sys::REC601YCB_CR_COLORSPACE => "Rec601YCbCr",
        sys::REC709YCB_CR_COLORSPACE => "Rec709YCbCr",
        sys::RGB_COLORSPACE => "RGB",
        sys::SCRGB_COLORSPACE => "scRGB",
        sys::SRGB_COLORSPACE => "sRGB",
        sys::TRANSPARENT_COLORSPACE => "Transparent",
        sys::XYY_COLORSPACE => "xyY",
        sys::XYZ_COLORSPACE => "XYZ",
        sys::YCBCR_COLORSPACE => "YCbCr",
        sys::YCC_COLORSPACE => "YCC",
        sys::YDBDR_COLORSPACE => "YDbDr",
        sys::YIQ_COLORSPACE => "YIQ",
        sys::YPBPR_COLORSPACE => "YPbPr",
        sys::YUV_COLORSPACE => "YUV",
        sys::LINEAR_GRAY_COLORSPACE => "LinearGray",
        sys::JZAZBZ_COLORSPACE => "Jzazbz",
        sys::DISPLAY_P3_COLORSPACE => "DisplayP3",
        sys::ADOBE98_COLORSPACE => "Adobe98",
        sys::PROPHOTO_COLORSPACE => "ProPhoto",
        sys::OKLAB_COLORSPACE => "Oklab",
        sys::OKLCH_COLORSPACE => "Oklch",
        _ => "Unknown",
    }
}

/// Resolves a colorspace name (case-insensitive, Pillow-style aliases accepted).
pub fn colorspace_from_name(name: &str) -> Option<u32> {
    lookup(name, COLORSPACES)
}

/// Resolves a channel name to its ImageMagick channel mask.
///
/// The mask itself is positional - `red` and `cyan` are both `0x0001`, meaning
/// "the first channel" - so this only maps a *name* onto a position. Whether the
/// image actually has that channel is a separate question, answered by
/// [`crate::image::Image::extract_channel`] against the image type.
pub fn channel_from_name(name: &str) -> Option<u32> {
    lookup(name, CHANNELS)
}

/// Every channel name magik accepts, in a stable order.
pub fn channel_names() -> impl Iterator<Item = &'static str> {
    CHANNELS.iter().map(|(name, _)| *name)
}

/// Canonical ImageMagick name for a `CompressionType` value.
pub fn compression_name(value: u32) -> &'static str {
    match value {
        sys::UNDEFINED_COMPRESSION => "Undefined",
        sys::B44A_COMPRESSION => "B44A",
        sys::B44_COMPRESSION => "B44",
        sys::BZIP_COMPRESSION => "BZip",
        sys::DXT1_COMPRESSION => "DXT1",
        sys::DXT3_COMPRESSION => "DXT3",
        sys::DXT5_COMPRESSION => "DXT5",
        sys::FAX_COMPRESSION => "Fax",
        sys::GROUP4_COMPRESSION => "Group4",
        sys::JBIG1_COMPRESSION => "JBIG1",
        sys::JBIG2_COMPRESSION => "JBIG2",
        sys::JPEG2000_COMPRESSION => "JPEG2000",
        sys::JPEG_COMPRESSION => "JPEG",
        sys::LOSSLESS_JPEG_COMPRESSION => "LosslessJPEG",
        sys::LZMA_COMPRESSION => "LZMA",
        sys::LZW_COMPRESSION => "LZW",
        sys::NO_COMPRESSION => "No",
        sys::PIZ_COMPRESSION => "PiZ",
        sys::PXR24_COMPRESSION => "Pxr24",
        sys::RLE_COMPRESSION => "RLE",
        sys::ZIP_COMPRESSION => "Zip",
        sys::ZIPS_COMPRESSION => "ZipS",
        sys::ZSTD_COMPRESSION => "Zstd",
        sys::WEBP_COMPRESSION => "WebP",
        sys::DWAA_COMPRESSION => "DWAA",
        sys::DWAB_COMPRESSION => "DWAB",
        sys::BC7_COMPRESSION => "BC7",
        sys::BC5_COMPRESSION => "BC5",
        sys::LERC_COMPRESSION => "LERC",
        _ => "Unknown",
    }
}

/// Resolves a compression name (case-insensitive).
pub fn compression_from_name(name: &str) -> Option<u32> {
    lookup(name, COMPRESSIONS)
}

/// Canonical ImageMagick name for an `ImageType` value.
pub fn image_type_name(value: u32) -> &'static str {
    match value {
        sys::UNDEFINED_TYPE => "Undefined",
        sys::BILEVEL_TYPE => "Bilevel",
        sys::GRAYSCALE_TYPE => "Grayscale",
        sys::GRAYSCALE_ALPHA_TYPE => "GrayscaleAlpha",
        sys::PALETTE_TYPE => "Palette",
        sys::PALETTE_ALPHA_TYPE => "PaletteAlpha",
        sys::TRUECOLOR_TYPE => "TrueColor",
        sys::TRUECOLOR_ALPHA_TYPE => "TrueColorAlpha",
        sys::COLOR_SEPARATION_TYPE => "ColorSeparation",
        sys::COLOR_SEPARATION_ALPHA_TYPE => "ColorSeparationAlpha",
        sys::OPTIMIZE_TYPE => "Optimize",
        sys::PALETTE_BILEVEL_ALPHA_TYPE => "PaletteBilevelAlpha",
        _ => "Unknown",
    }
}

/// Canonical ImageMagick name for a `FilterType` value.
pub fn filter_name(value: u32) -> &'static str {
    match value {
        sys::POINT_FILTER => "Point",
        sys::BOX_FILTER => "Box",
        sys::TRIANGLE_FILTER => "Triangle",
        sys::HERMITE_FILTER => "Hermite",
        sys::HANN_FILTER => "Hann",
        sys::HAMMING_FILTER => "Hamming",
        sys::BLACKMAN_FILTER => "Blackman",
        sys::GAUSSIAN_FILTER => "Gaussian",
        sys::QUADRATIC_FILTER => "Quadratic",
        sys::CUBIC_FILTER => "Cubic",
        sys::CATROM_FILTER => "Catrom",
        sys::MITCHELL_FILTER => "Mitchell",
        sys::JINC_FILTER => "Jinc",
        sys::SINC_FILTER => "Sinc",
        sys::SINC_FAST_FILTER => "SincFast",
        sys::KAISER_FILTER => "Kaiser",
        sys::WELCH_FILTER => "Welch",
        sys::PARZEN_FILTER => "Parzen",
        sys::BOHMAN_FILTER => "Bohman",
        sys::BARTLETT_FILTER => "Bartlett",
        sys::LAGRANGE_FILTER => "Lagrange",
        sys::LANCZOS_FILTER => "Lanczos",
        sys::LANCZOS_SHARP_FILTER => "LanczosSharp",
        sys::LANCZOS2_FILTER => "Lanczos2",
        sys::LANCZOS2_SHARP_FILTER => "Lanczos2Sharp",
        sys::ROBIDOUX_FILTER => "Robidoux",
        sys::ROBIDOUX_SHARP_FILTER => "RobidouxSharp",
        sys::COSINE_FILTER => "Cosine",
        sys::SPLINE_FILTER => "Spline",
        sys::LANCZOS_RADIUS_FILTER => "LanczosRadius",
        sys::CUBIC_SPLINE_FILTER => "CubicSpline",
        _ => "Unknown",
    }
}

/// Resolves a resampling-filter name (case-insensitive, aliases accepted).
pub fn filter_from_name(name: &str) -> Option<u32> {
    lookup(name, FILTERS)
}

/// The resampling filter magik uses when none is specified.
pub const DEFAULT_FILTER: u32 = sys::LANCZOS_FILTER;

/// Channel names mapped to their ImageMagick channel mask.
///
/// The masks are positional aliases in ImageMagick, so several names share a
/// value: `red`, `cyan` and `gray` are all "the first channel". Which of them is
/// valid depends on the image's type, which is checked separately.
const CHANNELS: &[(&str, u32)] = &[
    ("red", sys::RED_CHANNEL),
    ("r", sys::RED_CHANNEL),
    ("green", sys::GREEN_CHANNEL),
    ("g", sys::GREEN_CHANNEL),
    ("blue", sys::BLUE_CHANNEL),
    ("b", sys::BLUE_CHANNEL),
    ("alpha", sys::ALPHA_CHANNEL),
    ("a", sys::ALPHA_CHANNEL),
    ("cyan", sys::CYAN_CHANNEL),
    ("c", sys::CYAN_CHANNEL),
    ("magenta", sys::MAGENTA_CHANNEL),
    ("m", sys::MAGENTA_CHANNEL),
    ("yellow", sys::YELLOW_CHANNEL),
    ("y", sys::YELLOW_CHANNEL),
    ("black", sys::BLACK_CHANNEL),
    ("k", sys::BLACK_CHANNEL),
];

/// The channel *roles* a given ImageMagick image type actually carries.
///
/// Used to reject nonsense before it reaches the native call: asking a grayscale
/// image for "red", or an RGB image for "cyan", is an error rather than a
/// silently wrong answer.
pub fn available_channels(image_type: u32) -> &'static [&'static str] {
    match image_type {
        sys::BILEVEL_TYPE | sys::GRAYSCALE_TYPE => &[],
        sys::GRAYSCALE_ALPHA_TYPE | sys::PALETTE_ALPHA_TYPE | sys::PALETTE_BILEVEL_ALPHA_TYPE => {
            &["alpha"]
        }
        sys::PALETTE_TYPE => &[],
        sys::TRUECOLOR_TYPE | sys::OPTIMIZE_TYPE => &["red", "green", "blue"],
        sys::TRUECOLOR_ALPHA_TYPE => &["red", "green", "blue", "alpha"],
        sys::COLOR_SEPARATION_TYPE => &["cyan", "magenta", "yellow", "black"],
        sys::COLOR_SEPARATION_ALPHA_TYPE => &["cyan", "magenta", "yellow", "black", "alpha"],
        // Unknown types are not listed; the caller reports the type rather than
        // guessing which channels it might have.
        _ => &[],
    }
}

const COLORSPACES: &[(&str, u32)] = &[
    ("cmyk", sys::CMYK_COLORSPACE),
    ("cmy", sys::CMY_COLORSPACE),
    ("gray", sys::GRAY_COLORSPACE),
    ("grey", sys::GRAY_COLORSPACE),
    ("l", sys::GRAY_COLORSPACE),
    ("lineargray", sys::LINEAR_GRAY_COLORSPACE),
    ("hsl", sys::HSL_COLORSPACE),
    ("hsv", sys::HSV_COLORSPACE),
    ("hsb", sys::HSB_COLORSPACE),
    ("hwb", sys::HWB_COLORSPACE),
    ("lab", sys::LAB_COLORSPACE),
    ("lch", sys::LCH_COLORSPACE),
    ("luv", sys::LUV_COLORSPACE),
    ("ohta", sys::OHTA_COLORSPACE),
    ("rec601ycbcr", sys::REC601YCB_CR_COLORSPACE),
    ("rec709ycbcr", sys::REC709YCB_CR_COLORSPACE),
    ("rgb", sys::RGB_COLORSPACE),
    ("scrgb", sys::SCRGB_COLORSPACE),
    ("srgb", sys::SRGB_COLORSPACE),
    ("xyy", sys::XYY_COLORSPACE),
    ("xyz", sys::XYZ_COLORSPACE),
    ("ycbcr", sys::YCBCR_COLORSPACE),
    ("ycc", sys::YCC_COLORSPACE),
    ("ydbdr", sys::YDBDR_COLORSPACE),
    ("yiq", sys::YIQ_COLORSPACE),
    ("ypbpr", sys::YPBPR_COLORSPACE),
    ("yuv", sys::YUV_COLORSPACE),
    ("jzazbz", sys::JZAZBZ_COLORSPACE),
    ("displayp3", sys::DISPLAY_P3_COLORSPACE),
    ("adobe98", sys::ADOBE98_COLORSPACE),
    ("prophoto", sys::PROPHOTO_COLORSPACE),
    ("oklab", sys::OKLAB_COLORSPACE),
    ("oklch", sys::OKLCH_COLORSPACE),
];

const COMPRESSIONS: &[(&str, u32)] = &[
    ("b44a", sys::B44A_COMPRESSION),
    ("b44", sys::B44_COMPRESSION),
    ("bzip", sys::BZIP_COMPRESSION),
    ("dxt1", sys::DXT1_COMPRESSION),
    ("dxt3", sys::DXT3_COMPRESSION),
    ("dxt5", sys::DXT5_COMPRESSION),
    ("fax", sys::FAX_COMPRESSION),
    ("group4", sys::GROUP4_COMPRESSION),
    ("jbig1", sys::JBIG1_COMPRESSION),
    ("jbig2", sys::JBIG2_COMPRESSION),
    ("jpeg2000", sys::JPEG2000_COMPRESSION),
    ("jpeg", sys::JPEG_COMPRESSION),
    ("losslessjpeg", sys::LOSSLESS_JPEG_COMPRESSION),
    ("lzma", sys::LZMA_COMPRESSION),
    ("lzw", sys::LZW_COMPRESSION),
    ("no", sys::NO_COMPRESSION),
    ("none", sys::NO_COMPRESSION),
    ("piz", sys::PIZ_COMPRESSION),
    ("pxr24", sys::PXR24_COMPRESSION),
    ("rle", sys::RLE_COMPRESSION),
    ("zip", sys::ZIP_COMPRESSION),
    ("zips", sys::ZIPS_COMPRESSION),
    ("zstd", sys::ZSTD_COMPRESSION),
    ("webp", sys::WEBP_COMPRESSION),
    ("dwaa", sys::DWAA_COMPRESSION),
    ("dwab", sys::DWAB_COMPRESSION),
    ("bc7", sys::BC7_COMPRESSION),
    ("bc5", sys::BC5_COMPRESSION),
    ("lerc", sys::LERC_COMPRESSION),
];

const FILTERS: &[(&str, u32)] = &[
    ("point", sys::POINT_FILTER),
    ("box", sys::BOX_FILTER),
    ("triangle", sys::TRIANGLE_FILTER),
    ("hermite", sys::HERMITE_FILTER),
    ("hann", sys::HANN_FILTER),
    ("hamming", sys::HAMMING_FILTER),
    ("blackman", sys::BLACKMAN_FILTER),
    ("gaussian", sys::GAUSSIAN_FILTER),
    ("quadratic", sys::QUADRATIC_FILTER),
    ("cubic", sys::CUBIC_FILTER),
    ("catrom", sys::CATROM_FILTER),
    ("mitchell", sys::MITCHELL_FILTER),
    ("jinc", sys::JINC_FILTER),
    ("sinc", sys::SINC_FILTER),
    ("sincfast", sys::SINC_FAST_FILTER),
    ("kaiser", sys::KAISER_FILTER),
    ("welch", sys::WELCH_FILTER),
    ("parzen", sys::PARZEN_FILTER),
    ("bohman", sys::BOHMAN_FILTER),
    ("bartlett", sys::BARTLETT_FILTER),
    ("lagrange", sys::LAGRANGE_FILTER),
    ("lanczos", sys::LANCZOS_FILTER),
    ("lanczossharp", sys::LANCZOS_SHARP_FILTER),
    ("lanczos2", sys::LANCZOS2_FILTER),
    ("lanczos2sharp", sys::LANCZOS2_SHARP_FILTER),
    ("robidoux", sys::ROBIDOUX_FILTER),
    ("robidouxsharp", sys::ROBIDOUX_SHARP_FILTER),
    ("cosine", sys::COSINE_FILTER),
    ("spline", sys::SPLINE_FILTER),
    ("lanczosradius", sys::LANCZOS_RADIUS_FILTER),
    ("cubicspline", sys::CUBIC_SPLINE_FILTER),
];

/// Case-insensitive lookup in a `(lowercase name, value)` table.
fn lookup(name: &str, table: &[(&str, u32)]) -> Option<u32> {
    let key = name.trim().to_ascii_lowercase();
    table
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, value)| *value)
}
