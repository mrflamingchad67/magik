//! Safe ownership wrappers over the raw MagickWand handles.
//!
//! # Why this module exists
//!
//! `magik-core` must contain **no `unsafe` code**. That is only achievable if
//! something owns the raw pointers and guarantees their lifetime, and that
//! something has to be the crate that declares the FFI. So this module is the
//! narrow waist of the whole workspace: every `unsafe` block in
//! `magik-sys/src/` lives here, and `magik-core` deals only in the safe types
//! defined below.
//!
//! Nothing here interprets ImageMagick semantics — that is `magik-core`'s job.
//! This layer owns memory, converts C strings, and reports whether a C call
//! succeeded.
//!
//! # Threading
//!
//! [`Wand`] is [`Send`] but deliberately **not** [`Sync`]: ImageMagick allows a
//! wand to move between threads but not to be used from two threads at once.
//! `magik-core` obtains the missing `Sync` soundly by putting the wand behind a
//! mutex in its `Image` type.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_uchar};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Once;

use crate as ffi;
use crate::SizeType;

/// A failure reported by the MagickWand API.
///
/// The message is ImageMagick's own, copied out of the C allocation so it
/// outlives the call. `severity` is the raw `ExceptionType` value; classifying it
/// is `magik-core`'s job, because the mapping is policy rather than FFI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WandError {
    /// The verbatim ImageMagick message, when there was one.
    pub message: Option<String>,
    /// The raw `ExceptionType` ImageMagick recorded.
    pub severity: u32,
}

impl WandError {
    /// A failure with no ImageMagick message attached.
    pub fn bare(severity: u32) -> Self {
        Self {
            message: None,
            severity,
        }
    }
}

impl std::fmt::Display for WandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.message {
            Some(message) => write!(f, "{message}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for WandError {}

/// Result alias for the raw wrapper layer.
pub type Result<T> = std::result::Result<T, WandError>;

// ---------------------------------------------------------------------------
// Global initialisation
// ---------------------------------------------------------------------------

static GENESIS: Once = Once::new();

/// Initialise ImageMagick exactly once per process.
///
/// `MagickWandGenesis` is documented as idempotent, but a `Once` collapses
/// concurrent first-use into a single call.
///
/// # Note on `MagickWandTerminus`
///
/// It is deliberately **never** called. It tears down global ImageMagick state,
/// which would invalidate every live wand in the process — including wands owned
/// by other threads or by destructors running during interpreter shutdown.
/// Operating-system process teardown reclaims the memory anyway.
fn ensure_initialized() {
    GENESIS.call_once(|| {
        // SAFETY: takes no arguments, touches only global ImageMagick state, and
        // is documented as safe to call this way.
        unsafe { ffi::MagickWandGenesis() }
    });
}

// ---------------------------------------------------------------------------
// Small C-string helpers
// ---------------------------------------------------------------------------

/// Converts a Rust string into a NUL-terminated C string.
///
/// An interior NUL byte is reported as an error rather than silently truncating.
fn to_cstring(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| {
        WandError::bare(ffi::UNDEFINED_EXCEPTION).with_message(
            "value contains an interior NUL byte, which cannot be passed to ImageMagick",
        )
    })
}

impl WandError {
    /// Attaches a message, ignoring blanks.
    /// Attaches a message, ignoring blanks.
    ///
    /// Public because callers above this layer sometimes need to explain a
    /// condition ImageMagick reported without a reason of its own — most
    /// notably `MagickSetImageFormat`, which signals "unknown coder" by simply
    /// returning false and recording nothing.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        let message = message.into();
        if !message.trim().is_empty() {
            self.message = Some(message);
        }
        self
    }
}

/// Encodes a filesystem path for ImageMagick.
///
/// On Windows the path is widened to UTF-16 (the OS native form) and re-encoded
/// to UTF-8, which round-trips the full Unicode range apart from unpaired
/// surrogates. On Unix the path is passed through as raw bytes.
fn path_to_cstring(path: &Path) -> Result<CString> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        to_cstring(&String::from_utf16_lossy(&wide))
    }
    #[cfg(not(windows))]
    {
        to_cstring(&path.to_string_lossy())
    }
}

/// Takes ownership of a NUL-terminated string allocated by ImageMagick.
///
/// Returns `None` for a null pointer or a blank string.
unsafe fn take_cstring(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ImageMagick guarantees a NUL-terminated string; copy before free.
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `ptr` came from ImageMagick and is released exactly once.
    unsafe {
        ffi::MagickRelinquishMemory(ptr.cast());
    }
    (!text.trim().is_empty()).then_some(text)
}

/// Takes ownership of a byte buffer allocated by ImageMagick.
unsafe fn take_blob(ptr: *mut c_uchar, len: usize) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    if len == 0 {
        // SAFETY: the pointer came from ImageMagick, so it must be released even
        // though there is nothing to copy.
        unsafe {
            ffi::MagickRelinquishMemory(ptr.cast());
        }
        return Some(Vec::new());
    }
    // SAFETY: ImageMagick guarantees `len` readable bytes at `ptr`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    // SAFETY: `ptr` came from ImageMagick and is released exactly once.
    unsafe {
        ffi::MagickRelinquishMemory(ptr.cast());
    }
    Some(bytes)
}

/// Reads a static (non-owned) C string that takes an out-length parameter.
unsafe fn read_static_string(ptr: *const c_char, length: *mut usize) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: the caller guarantees a static NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    // SAFETY: the caller guarantees `length` is a valid, writable pointer.
    unsafe {
        *length = bytes.len();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Description of the linked ImageMagick build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagickVersion {
    /// Full version banner, e.g.
    /// `"ImageMagick 7.1.2-32 Q16-HDRI x64 ..."`.
    pub version: String,
    /// Quantum depth token, e.g. `"Q16"`.
    pub quantum_depth: String,
    /// Quantum range description, e.g. `"65535"`.
    pub quantum_range: String,
}

/// Information about the ImageMagick build this binary is linked to.
pub fn version() -> MagickVersion {
    ensure_initialized();
    let mut len: usize = 0;
    // SAFETY: all three return static strings and only write to the supplied
    // length out-parameter, which is a valid local.
    let (version, quantum_depth, quantum_range) = unsafe {
        (
            read_static_string(ffi::MagickGetVersion(&mut len), &mut len),
            read_static_string(ffi::MagickGetQuantumDepth(&mut len), &mut len),
            read_static_string(ffi::MagickGetQuantumRange(&mut len), &mut len),
        )
    };
    MagickVersion {
        version,
        quantum_depth,
        quantum_range,
    }
}

// ---------------------------------------------------------------------------
// Pixel wand
// ---------------------------------------------------------------------------

/// An owned `PixelWand` (a single colour value), released on drop.
pub struct PixelColor {
    ptr: NonNull<ffi::PixelWand>,
}

impl PixelColor {
    /// Creates a pixel wand set to an ImageMagick colour specification such as
    /// `"none"`, `"white"`, `"#ff0000"` or `"rgb(1,2,3)"`.
    pub fn new(spec: &str) -> Result<Self> {
        // SAFETY: `NewPixelWand` takes no arguments; the NULL result is handled
        // immediately and the wand is owned by the returned guard.
        let raw = unsafe { ffi::NewPixelWand() };
        let ptr = NonNull::new(raw).ok_or_else(|| {
            WandError::bare(ffi::UNDEFINED_EXCEPTION).with_message("NewPixelWand returned NULL")
        })?;
        let this = Self { ptr };
        let spec_c = to_cstring(spec)?;
        // SAFETY: `ptr` is a fresh, live pixel wand; `spec_c` outlives the call.
        let ok = unsafe { ffi::PixelSetColor(this.ptr.as_ptr(), spec_c.as_ptr()) };
        if ok != ffi::MAGICK_TRUE {
            return Err(WandError::bare(ffi::UNDEFINED_EXCEPTION)
                .with_message(format!("invalid colour specification {spec:?}")));
        }
        Ok(this)
    }

    /// Renders the colour as an ImageMagick colour string.
    pub fn to_color_string(&self) -> Result<String> {
        // SAFETY: `self.ptr` is live; the returned string is copied and freed.
        let raw = unsafe { take_cstring(ffi::PixelGetColorAsString(self.ptr.as_ptr())) };
        raw.ok_or_else(|| {
            WandError::bare(ffi::UNDEFINED_EXCEPTION)
                .with_message("PixelGetColorAsString produced no colour")
        })
    }

    fn as_ptr(&self) -> *mut ffi::PixelWand {
        self.ptr.as_ptr()
    }
}

impl Drop for PixelColor {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from `NewPixelWand` and is released exactly once.
        unsafe {
            ffi::DestroyPixelWand(self.ptr.as_ptr());
        }
    }
}

// ---------------------------------------------------------------------------
// Wand
// ---------------------------------------------------------------------------

/// A safe, owning handle to a `MagickWand`.
///
/// Allocated by [`Wand::new`] and destroyed on drop; copies come from
/// [`Wand::try_clone`], which performs a real ImageMagick deep copy.
pub struct Wand {
    ptr: NonNull<ffi::MagickWand>,
}

// SAFETY: a `MagickWand` owns its image list independently and may be
// transferred between threads, provided no two threads use the *same* wand at
// once. `Sync` is intentionally withheld so the compiler prevents that.
unsafe impl Send for Wand {}

impl Wand {
    /// Allocates a new, empty wand.
    pub fn new() -> Result<Self> {
        ensure_initialized();
        // SAFETY: no preconditions; the result is checked for NULL below.
        let raw = unsafe { ffi::NewMagickWand() };
        let ptr = NonNull::new(raw).ok_or_else(|| {
            WandError::bare(ffi::UNDEFINED_EXCEPTION).with_message("NewMagickWand returned NULL")
        })?;
        Ok(Self { ptr })
    }

    /// Deep-copies the wand and its images.
    pub fn try_clone(&self) -> Result<Self> {
        // SAFETY: `self.ptr` is live for the duration of the call.
        let raw = unsafe { ffi::CloneMagickWand(self.ptr.as_ptr()) };
        NonNull::new(raw).map(|ptr| Self { ptr }).ok_or_else(|| {
            WandError::bare(ffi::UNDEFINED_EXCEPTION).with_message("CloneMagickWand returned NULL")
        })
    }

    /// Releases the wand's images, leaving an empty but usable wand.
    pub fn clear(&self) {
        // SAFETY: `self.ptr` is live.
        unsafe {
            ffi::ClearMagickWand(self.ptr.as_ptr());
        }
    }

    fn as_ptr(&self) -> *mut ffi::MagickWand {
        self.ptr.as_ptr()
    }

    /// Reads the pending ImageMagick exception, if any.
    pub fn last_error(&self) -> WandError {
        let mut severity: ffi::ExceptionType = ffi::UNDEFINED_EXCEPTION;
        // SAFETY: `self.ptr` is live and `severity` is a valid local; the
        // returned string is copied out and freed before returning.
        let message =
            unsafe { take_cstring(ffi::MagickGetException(self.ptr.as_ptr(), &mut severity)) };
        WandError { message, severity }
    }

    /// Turns a `MagickBooleanType` result into a [`Result`].
    ///
    /// On failure the wand's pending exception is read and returned **as
    /// ImageMagick phrased it**. This layer deliberately does *not* invent a
    /// message when ImageMagick recorded none, and it does not name the C
    /// function that failed: a fabricated reason such as
    /// "MagickSetImageFormat failed" reads like a diagnosis while telling the
    /// caller nothing, and it would hide the fact that no real one exists.
    /// Several MagickWand calls — `MagickSetImageFormat` among them — signal
    /// failure by returning false and recording nothing at all. Deciding what to
    /// tell the user is [`crate::wand`] callers' job, above this layer.
    fn check(&self, status: ffi::MagickBooleanType) -> Result<()> {
        if status != ffi::MAGICK_FALSE {
            return Ok(());
        }
        Err(self.last_error())
    }

    // -- input -------------------------------------------------------------

    /// Loads an image from `path`.
    pub fn read_path(&self, path: &Path) -> Result<()> {
        let c_path = path_to_cstring(path)?;
        // SAFETY: live wand; `c_path` is a valid NUL-terminated string that
        // outlives the call.
        let status = unsafe { ffi::MagickReadImage(self.as_ptr(), c_path.as_ptr()) };
        self.check(status)
    }

    /// Loads an image from an in-memory buffer.
    ///
    /// The buffer is borrowed for the duration of the call only; no copy is made.
    pub fn read_blob(&self, data: &[u8]) -> Result<()> {
        // SAFETY: live wand; `data` is a valid slice of exactly `data.len()`
        // bytes, which is the length reported.
        let status =
            unsafe { ffi::MagickReadImageBlob(self.as_ptr(), data.as_ptr().cast(), data.len()) };
        self.check(status)
    }

    // -- output ------------------------------------------------------------

    /// Writes the wand's images to `path`.
    ///
    /// When `format` is given it is applied through ImageMagick's coder prefix
    /// (`"PNG:out.jpg"`) rather than by setting the wand's format, because
    /// `MagickWriteImage` gives the *filename extension* precedence over the
    /// wand's format. The wand's own format is never modified.
    pub fn write_path(&self, path: &Path, format: Option<&str>) -> Result<()> {
        let target = match format {
            Some(coder) => format!("{coder}:{}", path.to_string_lossy()),
            None => path.to_string_lossy().into_owned(),
        };
        let c_target = to_cstring(&target)?;
        // SAFETY: live wand; `c_target` is valid for the call.
        let status = unsafe { ffi::MagickWriteImage(self.as_ptr(), c_target.as_ptr()) };
        self.check(status)
    }

    /// Encodes the wand's images into a newly allocated buffer.
    pub fn write_blob(&self) -> Result<Vec<u8>> {
        let mut len: usize = 0;
        // SAFETY: live wand; `len` is a valid out-parameter. The buffer is copied
        // into a `Vec` and released before returning.
        let blob = unsafe { take_blob(ffi::MagickGetImageBlob(self.as_ptr(), &mut len), len) };
        if let Some(bytes) = blob {
            return Ok(bytes);
        }
        // `MagickGetImageBlob` reports failure by returning NULL, so the reason
        // only exists in the wand's pending exception. It must be read here or
        // the caller sees a bare failure with no ImageMagick detail.
        let detail = self.last_error();
        Err(if detail.message.is_some() {
            detail
        } else {
            WandError::bare(detail.severity).with_message("MagickGetImageBlob produced no data")
        })
    }

    // -- properties --------------------------------------------------------

    /// Width in pixels of the wand's current image (`0` when empty).
    pub fn width(&self) -> u32 {
        // SAFETY: live wand.
        saturating_u32(unsafe { ffi::MagickGetImageWidth(self.as_ptr()) })
    }

    /// Height in pixels of the wand's current image (`0` when empty).
    pub fn height(&self) -> u32 {
        // SAFETY: live wand.
        saturating_u32(unsafe { ffi::MagickGetImageHeight(self.as_ptr()) })
    }

    /// Bits of precision per colour channel (e.g. `8` or `16`).
    pub fn depth(&self) -> u32 {
        // SAFETY: live wand.
        saturating_u32(unsafe { ffi::MagickGetImageDepth(self.as_ptr()) })
    }

    /// Raw `ColorspaceType` value.
    pub fn colorspace(&self) -> u32 {
        // SAFETY: live wand.
        unsafe { ffi::MagickGetImageColorspace(self.as_ptr()) }
    }

    /// Raw `CompressionType` value.
    pub fn compression(&self) -> u32 {
        // SAFETY: live wand.
        unsafe { ffi::MagickGetImageCompression(self.as_ptr()) }
    }

    /// Encoder quality hint in `0..=100`.
    pub fn compression_quality(&self) -> u32 {
        // SAFETY: live wand.
        saturating_u32(unsafe { ffi::MagickGetImageCompressionQuality(self.as_ptr()) })
    }

    /// ImageMagick's structural classification of the image.
    pub fn image_type(&self) -> u32 {
        // SAFETY: live wand.
        unsafe { ffi::MagickGetImageType(self.as_ptr()) }
    }

    /// Whether the image carries an alpha channel.
    pub fn has_alpha(&self) -> bool {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickGetImageAlphaChannel(self.as_ptr()) };
        status == ffi::MAGICK_TRUE
    }

    /// The image's magick format, e.g. `"PNG"`.
    pub fn format(&self) -> Result<String> {
        // SAFETY: live wand; the returned string is copied and freed.
        let raw = unsafe { take_cstring(ffi::MagickGetImageFormat(self.as_ptr())) };
        raw.map(|value| value.trim().to_ascii_uppercase())
            .ok_or_else(|| {
                WandError::bare(ffi::UNDEFINED_EXCEPTION)
                    .with_message("ImageMagick reported no format for this image")
            })
    }

    // -- settings ----------------------------------------------------------

    /// Selects the encoder used when this image is written.
    pub fn set_format(&self, format: &str) -> Result<()> {
        let c_format = to_cstring(format)?;
        // SAFETY: live wand; `c_format` valid for the call.
        let status = unsafe { ffi::MagickSetImageFormat(self.as_ptr(), c_format.as_ptr()) };
        self.check(status)
    }

    /// Selects the image's colorspace without converting the pixels.
    pub fn set_colorspace(&self, colorspace: u32) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickSetImageColorspace(self.as_ptr(), colorspace) };
        self.check(status)
    }

    /// Selects the compression scheme used when the image is encoded.
    pub fn set_compression(&self, compression: u32) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickSetImageCompression(self.as_ptr(), compression) };
        self.check(status)
    }

    /// Sets the encoder quality hint (`0..=100`).
    pub fn set_compression_quality(&self, quality: u32) -> Result<()> {
        // SAFETY: live wand.
        let status =
            unsafe { ffi::MagickSetImageCompressionQuality(self.as_ptr(), quality as usize) };
        self.check(status)
    }

    /// Forces the image's structural classification.
    pub fn set_image_type(&self, image_type: u32) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickSetImageType(self.as_ptr(), image_type) };
        self.check(status)
    }

    /// Activates, removes or shapes the alpha channel.
    pub fn set_alpha_channel(&self, option: u32) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickSetImageAlphaChannel(self.as_ptr(), option) };
        self.check(status)
    }

    /// Reads a coder/wand option such as `"png:color-type"`.
    ///
    /// Returns `None` when the option is not set.
    pub fn get_option(&self, key: &str) -> Option<String> {
        let c_key = to_cstring(key).ok()?;
        // SAFETY: live wand; `c_key` valid for the call; result copied and freed.
        unsafe { take_cstring(ffi::MagickGetOption(self.as_ptr(), c_key.as_ptr())) }
    }

    /// Sets a coder/wand option such as `"jpeg:sampling-factor"`.
    pub fn set_option(&self, key: &str, value: &str) -> Result<()> {
        let c_key = to_cstring(key)?;
        let c_value = to_cstring(value)?;
        // SAFETY: live wand; both strings valid for the call.
        let status =
            unsafe { ffi::MagickSetOption(self.as_ptr(), c_key.as_ptr(), c_value.as_ptr()) };
        self.check(status)
    }

    // -- transformations ---------------------------------------------------

    /// Resamples the image to `width` x `height`.
    pub fn resize(&self, width: u32, height: u32, filter: u32) -> Result<()> {
        // SAFETY: live wand; dimensions are validated by the caller.
        let status = unsafe {
            ffi::MagickResizeImage(self.as_ptr(), width as usize, height as usize, filter)
        };
        self.check(status)
    }

    /// Extracts the rectangle `(left, top)` .. `(right, bottom)`.
    pub fn crop(&self, left: i64, top: i64, right: i64, bottom: i64) -> Result<()> {
        let width = right - left;
        let height = bottom - top;
        // `MagickCropImage` takes `ssize_t` offsets, narrower than the `i64` this
        // layer accepts, so convert explicitly rather than truncating.
        let x = isize::try_from(left).map_err(|_| {
            WandError::bare(ffi::TYPE_ERROR).with_message("crop origin is out of range")
        })?;
        let y = isize::try_from(top).map_err(|_| {
            WandError::bare(ffi::TYPE_ERROR).with_message("crop origin is out of range")
        })?;
        // SAFETY: live wand; dimensions validated by the caller and non-negative.
        let status =
            unsafe { ffi::MagickCropImage(self.as_ptr(), width as usize, height as usize, x, y) };
        self.check(status)
    }

    /// Rotates by `degrees` clockwise, filling exposed areas with `background`.
    pub fn rotate(&self, degrees: f64, background: &str) -> Result<()> {
        let bg = PixelColor::new(background)?;
        // SAFETY: live wand; `bg` is a live pixel wand that outlives the call.
        let status = unsafe { ffi::MagickRotateImage(self.as_ptr(), bg.as_ptr(), degrees) };
        self.check(status)
    }

    /// Mirrors the image vertically (top ↔ bottom).
    pub fn flip(&self) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickFlipImage(self.as_ptr()) };
        self.check(status)
    }

    /// Mirrors the image horizontally (left ↔ right).
    pub fn flop(&self) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickFlopImage(self.as_ptr()) };
        self.check(status)
    }

    /// Applies a Gaussian blur.
    pub fn blur(&self, radius: f64, sigma: f64) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickBlurImage(self.as_ptr(), radius, sigma) };
        self.check(status)
    }

    /// Converts the image into another colorspace.
    ///
    /// Unlike [`Wand::set_colorspace`], this rewrites the pixel values; it is a
    /// genuine transform rather than a relabelling.
    pub fn transform_colorspace(&self, colorspace: u32) -> Result<()> {
        // SAFETY: live wand.
        let status = unsafe { ffi::MagickTransformImageColorspace(self.as_ptr(), colorspace) };
        self.check(status)
    }

    /// Replaces the image with a single channel.
    ///
    /// `channel` is a [`ffi::ChannelType`] bitmask. **The caller is responsible
    /// for validating that the mask names a channel this image actually has**:
    /// the ImageMagick constants are positional aliases, so `RED_CHANNEL` and
    /// `CYAN_CHANNEL` are both `0x0001` and mean "the first channel" rather
    /// than "red" or "cyan" specifically. Validating here would duplicate policy
    /// that belongs above this layer.
    pub fn separate_channel(&self, channel: u32) -> Result<()> {
        // SAFETY: live wand; `channel` is a plain integer bitmask.
        let status = unsafe { ffi::MagickSeparateImage(self.as_ptr(), channel) };
        self.check(status)
    }

    /// Quantises the image to `depth` bits per sample.
    ///
    /// This genuinely discards precision: sample values are requantised to the
    /// requested depth, so information above it cannot be recovered afterwards.
    pub fn set_depth(&self, depth: u32) -> Result<()> {
        // SAFETY: live wand; `depth` is a plain integer.
        let status = unsafe { ffi::MagickSetImageDepth(self.as_ptr(), depth as SizeType) };
        self.check(status)
    }

    // -- construction ------------------------------------------------------

    /// Creates a `width` x `height` image filled with `background`.
    pub fn new_image(&self, width: u32, height: u32, background: &PixelColor) -> Result<()> {
        // SAFETY: live wand; `background` is a live pixel wand that outlives the
        // call.
        let status = unsafe {
            ffi::MagickNewImage(
                self.as_ptr(),
                width as usize,
                height as usize,
                background.as_ptr(),
            )
        };
        self.check(status)
    }
}

// ---------------------------------------------------------------------------
// Pixel access
// ---------------------------------------------------------------------------

/// Channel-map characters magik accepts.
///
/// ImageMagick's `SetPixelChannelMap` — which interprets the `map` argument — is
/// an internal function with no header declaration, and it does **not** validate
/// its input: an unrecognised string silently degrades to a CMYK map and then
/// fails with a confusing "color separated image required". magik therefore
/// restricts the map to the characters ImageMagick actually honours, which were
/// confirmed empirically against a Q16-HDRI build:
///
/// | map    | samples | meaning                    |
/// |--------|---------|----------------------------|
/// | `R`    | 1       | red / gray / bilevel level |
/// | `RGB`  | 3       | red, green, blue           |
/// | `RGBA` | 4       | plus alpha                 |
/// | `CMYK` | 4       | cyan, magenta, yellow, key |
///
/// Note that `GRAY` is **not** valid here despite being a plausible guess.
fn validate_map(map: &str) -> Result<usize> {
    let mut samples = 0usize;
    for ch in map.chars() {
        if !matches!(
            ch.to_ascii_uppercase(),
            'R' | 'G' | 'B' | 'A' | 'C' | 'M' | 'Y' | 'K'
        ) {
            return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
                "unsupported pixel channel map {map:?}: only R, G, B, A, C, M, Y and K are valid"
            )));
        }
        samples += 1;
    }
    if samples == 0 {
        return Err(WandError::bare(ffi::TYPE_ERROR)
            .with_message("pixel channel map must name at least one channel"));
    }
    // ImageMagick resolves a one-character map to the red channel, which is also
    // how a grayscale or bilevel image's level is read. Requiring `R` here
    // catches the natural but wrong guess `"GRAY"` (which ImageMagick would
    // silently reinterpret as a four-channel CMYK map).
    if samples == 1 && !map.eq_ignore_ascii_case("R") {
        return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
            "a single-channel pixel map must be \"R\", not {map:?}"
        )));
    }
    Ok(samples)
}

/// Total sample count a region transfer needs.
fn required_samples(columns: u32, rows: u32, channels: usize) -> Result<usize> {
    (columns as usize)
        .checked_mul(rows as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| {
            WandError::bare(ffi::TYPE_ERROR)
                .with_message("pixel region size overflows the addressable range")
        })
}

/// Narrows a C `size_t` dimension to `u32` without wrapping.
fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

impl Wand {
    /// Copies a rectangular region of 8-bit samples out of the image.
    ///
    /// `out` must be at least `columns * rows * map.len()` bytes; a shorter
    /// buffer is rejected **before** the FFI call, because ImageMagick writes
    /// the full extent regardless and would otherwise overrun it.
    pub fn export_pixels_u8(
        &self,
        x: i64,
        y: i64,
        columns: u32,
        rows: u32,
        map: &str,
        out: &mut [u8],
    ) -> Result<()> {
        let channels = validate_map(map)?;
        let needed = required_samples(columns, rows, channels)?;
        if out.len() < needed {
            return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
                "destination buffer holds {} byte(s) but a {columns}x{rows} {map} region needs {needed}",
                out.len()
            )));
        }
        let c_map = to_cstring(map)?;
        // SAFETY: live wand; `c_map` is valid for the call; `out` is a mutable
        // slice whose length has just been checked to cover the full extent
        // ImageMagick will write.
        let status = unsafe {
            ffi::MagickExportImagePixels(
                self.as_ptr(),
                x as isize,
                y as isize,
                columns as usize,
                rows as usize,
                c_map.as_ptr(),
                ffi::CHAR_PIXEL,
                out.as_mut_ptr().cast(),
            )
        };
        self.check(status)
    }

    /// Copies a rectangular region of 16-bit samples out of the image.
    ///
    /// `out` must hold at least `columns * rows * map.len()` `u16` samples, in
    /// native (little) endian order.
    pub fn export_pixels_u16(
        &self,
        x: i64,
        y: i64,
        columns: u32,
        rows: u32,
        map: &str,
        out: &mut [u16],
    ) -> Result<()> {
        let channels = validate_map(map)?;
        let needed = required_samples(columns, rows, channels)?;
        if out.len() < needed {
            return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
                "destination buffer holds {} sample(s) but a {columns}x{rows} {map} region needs {needed}",
                out.len()
            )));
        }
        let c_map = to_cstring(map)?;
        // SAFETY: live wand; `c_map` is valid for the call. `u16` slices are
        // contiguous two-byte samples, which is exactly the `SHORT_PIXEL` layout,
        // so the pointer cast preserves alignment and the length check above
        // covers the full extent ImageMagick writes.
        let status = unsafe {
            ffi::MagickExportImagePixels(
                self.as_ptr(),
                x as isize,
                y as isize,
                columns as usize,
                rows as usize,
                c_map.as_ptr(),
                ffi::SHORT_PIXEL,
                out.as_mut_ptr().cast(),
            )
        };
        self.check(status)
    }

    /// Replaces a rectangular region with 8-bit samples.
    ///
    /// The region is **overwritten**, not blended.
    pub fn import_pixels_u8(
        &self,
        x: i64,
        y: i64,
        columns: u32,
        rows: u32,
        map: &str,
        data: &[u8],
    ) -> Result<()> {
        let channels = validate_map(map)?;
        let needed = required_samples(columns, rows, channels)?;
        if data.len() < needed {
            return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
                "source buffer holds {} byte(s) but a {columns}x{rows} {map} region needs {needed}",
                data.len()
            )));
        }
        let c_map = to_cstring(map)?;
        // SAFETY: live wand; `c_map` valid for the call; `data` is a readable
        // slice whose length has just been checked to cover the full extent
        // ImageMagick reads.
        let status = unsafe {
            ffi::MagickImportImagePixels(
                self.as_ptr(),
                x as isize,
                y as isize,
                columns as usize,
                rows as usize,
                c_map.as_ptr(),
                ffi::CHAR_PIXEL,
                data.as_ptr().cast(),
            )
        };
        self.check(status)
    }

    /// Replaces a rectangular region with 16-bit samples.
    ///
    /// The region is **overwritten**, not blended. Samples are read in native
    /// (little) endian order.
    pub fn import_pixels_u16(
        &self,
        x: i64,
        y: i64,
        columns: u32,
        rows: u32,
        map: &str,
        data: &[u16],
    ) -> Result<()> {
        let channels = validate_map(map)?;
        let needed = required_samples(columns, rows, channels)?;
        if data.len() < needed {
            return Err(WandError::bare(ffi::TYPE_ERROR).with_message(format!(
                "source buffer holds {} sample(s) but a {columns}x{rows} {map} region needs {needed}",
                data.len()
            )));
        }
        let c_map = to_cstring(map)?;
        // SAFETY: live wand; `c_map` valid for the call; `u16` slices are
        // contiguous two-byte samples matching `SHORT_PIXEL`, and the length
        // check above covers the full extent ImageMagick reads.
        let status = unsafe {
            ffi::MagickImportImagePixels(
                self.as_ptr(),
                x as isize,
                y as isize,
                columns as usize,
                rows as usize,
                c_map.as_ptr(),
                ffi::SHORT_PIXEL,
                data.as_ptr().cast(),
            )
        };
        self.check(status)
    }

    /// Reads the colour of the single pixel at `(x, y)`.
    ///
    /// Note that ImageMagick does **not** bounds-check `(x, y)`: an
    /// out-of-range coordinate silently returns an unrelated pixel, so callers
    /// must validate coordinates themselves.
    pub fn pixel_color(&self, x: i64, y: i64) -> Result<PixelColor> {
        // SAFETY: `NewPixelWand` takes no arguments; NULL is handled below.
        let raw = unsafe { ffi::NewPixelWand() };
        let ptr = NonNull::new(raw).ok_or_else(|| {
            WandError::bare(ffi::UNDEFINED_EXCEPTION).with_message("NewPixelWand returned NULL")
        })?;
        let color = PixelColor { ptr };
        // SAFETY: `self.ptr` is live and `color.ptr` is a live pixel wand.
        let status = unsafe {
            ffi::MagickGetImagePixelColor(self.as_ptr(), x as isize, y as isize, color.as_ptr())
        };
        self.check(status)?;
        Ok(color)
    }

    /// Sets the colour of the single pixel at `(x, y)`.
    ///
    /// As with [`Wand::pixel_color`], ImageMagick does not bounds-check the
    /// coordinate.
    pub fn set_pixel_color(&self, x: i64, y: i64, color: &PixelColor) -> Result<()> {
        // SAFETY: `self.ptr` is live and `color.ptr` is a live pixel wand.
        let status = unsafe {
            ffi::MagickSetImagePixelColor(self.as_ptr(), x as isize, y as isize, color.as_ptr())
        };
        self.check(status)
    }
}

impl Drop for Wand {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from `NewMagickWand`/`CloneMagickWand` and is
        // destroyed exactly once, here.
        unsafe {
            ffi::DestroyMagickWand(self.ptr.as_ptr());
        }
    }
}

impl std::fmt::Debug for Wand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = (self.width(), self.height());
        f.debug_struct("Wand").field("size", &(w, h)).finish()
    }
}
