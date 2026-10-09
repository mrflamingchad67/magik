//! Stage 03 tests: loading, saving, format selection, and error propagation.
//!
//! These exercise `magik-core`'s I/O layer directly, with no Python in the
//! picture, which is the whole point of the crate layering: the safe layer is
//! testable on its own.

use std::path::{Path, PathBuf};

use magik_core::error::Result;
use magik_core::magick::image_type_name;
use magik_core::pixel::mode_for_image;
use magik_core::{Error, ErrorKind, Image, PixelMode, SampleDepth};
use magik_sys as sys;

// -- fixtures ---------------------------------------------------------------

const W: u32 = 16;
const H: u32 = 12;

/// A scratch directory under the OS temp dir, removed when the guard drops.
///
/// The crate has no tempfile dependency and Stage 03 adds none, so the cleanup
/// is done by hand.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("magik-io-{tag}-{unique}"));
        std::fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.outer().join(name)
    }

    fn outer(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn image() -> Image {
    Image::new(PixelMode::Rgb, (W, H), "black", SampleDepth::Eight).expect("image")
}

/// An image with enough colour variety to resist ImageMagick's palette rewrite.
///
/// Flat images are useless for testing container fidelity: ImageMagick shrinks
/// them to a palette or bilevel PNG, which changes the stored type (and so the
/// number of samples `pixels` returns) without changing a single colour.
fn detailed() -> Image {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            data.push((x * 37 + y * 11) as u8);
            data.push((x * 5 + y * 29) as u8);
            data.push((x * 13 + y * 7) as u8);
        }
    }
    Image::new(PixelMode::Rgb, (W, H), "black", SampleDepth::Eight)
        .expect("base")
        .putpixels((0, 0), (W, H), &data, SampleDepth::Eight)
        .expect("fill")
}

/// Formats this ImageMagick build can write, probed rather than assumed.
fn writable(format: &str) -> bool {
    image().write_bytes(Some(format)).is_ok()
}

// -- §4 format selection ---------------------------------------------------

#[test]
fn explicit_format_overrides_the_extension() {
    let scratch = Scratch::new("override");
    let out = scratch.join("misleading.jpg");
    image().save_with_format(&out, "PNG").expect("save as png");
    assert!(
        std::fs::read(&out)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n"),
        "an explicit PNG format must win over a .jpg extension"
    );
}

#[test]
fn extension_selects_the_encoder_when_no_format_is_given() {
    let scratch = Scratch::new("extension");
    let out = scratch.join("by-extension.webp");
    if !writable("WEBP") {
        return; // delegate absent in this build
    }
    image().save(&out).expect("save");
    let reopened = Image::from_bytes(&std::fs::read(&out).unwrap()).expect("reopen");
    assert_eq!(reopened.format().unwrap(), "WEBP");
}

#[test]
fn format_names_are_case_insensitive() {
    let scratch = Scratch::new("case");
    let lower = scratch.join("lower.png");
    let upper = scratch.join("upper.png");
    image().save_with_format(&lower, "png").expect("lower");
    image().save_with_format(&upper, "PNG").expect("upper");
    assert_eq!(
        std::fs::read(&lower).unwrap(),
        std::fs::read(&upper).unwrap()
    );
}

#[test]
fn saving_leaves_the_images_own_format_alone() {
    let img = image();
    let before = img.format().unwrap();
    let scratch = Scratch::new("identity");
    for name in ["a.png", "b.miff"] {
        let _ = img.save(scratch.join(name));
    }
    let _ = img.write_bytes(Some("JPEG"));
    assert_eq!(img.format().unwrap(), before);
}

// -- the format-validation bug ---------------------------------------------

#[test]
fn an_unknown_format_is_rejected_rather_than_ignored() {
    let scratch = Scratch::new("unknown");
    let out = scratch.join("out.png");
    let error = image()
        .save_with_format(&out, "NOT-A-FORMAT")
        .expect_err("an unknown coder must not be accepted");
    assert_eq!(error.kind(), ErrorKind::Format);
    assert!(
        !out.exists(),
        "a rejected format must not leave a file behind"
    );
}

#[test]
fn an_unknown_format_never_falls_back_to_the_extension() {
    // ImageMagick's writer ignores a coder prefix it does not recognise and
    // silently uses the filename extension instead. Before magik validated the
    // format, `save_with_format(path, "NOT-A-FORMAT")` therefore wrote a valid
    // PNG and reported success — a silently wrong file rather than an error.
    let scratch = Scratch::new("fallback");
    for bad in ["NOT-A-FORMAT", "BOGUSFMT", "J", "12"] {
        let out = scratch.join(&format!("{bad}.png"));
        assert!(
            image().save_with_format(&out, bad).is_err(),
            "{bad} was accepted and may have written the wrong format"
        );
        assert!(!out.exists(), "{bad} wrote a file anyway");
    }
}

#[test]
fn the_unknown_format_message_does_not_just_name_a_c_function() {
    let error = image()
        .write_bytes(Some("NOT-A-FORMAT"))
        .expect_err("unknown format");
    let message = error.to_string();
    assert!(
        !message.contains("MagickSetImageFormat"),
        "the message must explain, not name the C function: {message}"
    );
    assert!(message.contains("NOT-A-FORMAT"), "{message}");
}

#[test]
fn blank_format_names_are_rejected() {
    for blank in ["", "   ", "\t"] {
        let error = image().write_bytes(Some(blank)).expect_err("blank format");
        assert_eq!(error.kind(), ErrorKind::Format);
    }
}

#[test]
fn a_failed_save_still_restores_the_original_format() {
    let img = image();
    let before = img.format().unwrap();
    assert!(img.write_bytes(Some("NOT-A-FORMAT")).is_err());
    assert_eq!(img.format().unwrap(), before);
}

// -- §12 error propagation -------------------------------------------------

#[test]
fn an_unencodable_format_is_reported_as_unsupported() {
    let error = image()
        .write_bytes(Some("XCF"))
        .expect_err("XCF should not be writable");
    // Either "unsupported format" or a save failure, depending on the build;
    // what must never happen is a silent success.
    assert!(
        matches!(
            error.kind(),
            ErrorKind::Format | ErrorKind::UnsupportedFormat | ErrorKind::Save
        ),
        "unexpected kind {:?}",
        error.kind()
    );
}

#[test]
fn a_missing_input_path_reports_an_open_error() {
    let error = Image::open(Path::new("/definitely/not/here.png")).expect_err("missing file");
    assert_eq!(error.kind(), ErrorKind::Open);
}

#[test]
fn invalid_input_bytes_never_yield_an_empty_image() {
    for payload in [&b""[..], &b"\x00"[..], &b"garbage"[..]] {
        assert!(Image::from_bytes(payload).is_err(), "accepted {payload:?}");
    }
}

#[test]
fn an_unwritable_destination_is_a_save_error() {
    let scratch = Scratch::new("unwritable");
    let bad = scratch.join("missing-dir").join("out.png");
    let error = image()
        .save(&bad)
        .expect_err("save into a missing directory");
    assert_eq!(error.kind(), ErrorKind::Save);
}

// -- §10 round trips -------------------------------------------------------

#[test]
fn lossless_containers_preserve_every_sample() {
    for format in ["PNG", "TIFF", "BMP"] {
        if !writable(format) {
            continue;
        }
        let encoded = detailed().write_bytes(Some(format)).expect("encode");
        let restored = Image::from_bytes(&encoded).expect("decode");
        assert_eq!(restored.size(), (W, H), "{format} changed the size");
        assert_eq!(
            restored.pixels(SampleDepth::Eight).unwrap(),
            detailed().pixels(SampleDepth::Eight).unwrap(),
            "{format} altered the pixels"
        );
    }
}

#[test]
fn a_flat_image_keeps_its_colour_but_not_its_sample_count() {
    // A single-colour image is the extreme case of ImageMagick's PNG
    // optimisation: it becomes a 1-bit or greyscale PNG, so `pixels` returns
    // fewer samples than the three it went in with.
    //
    // This is correct rather than lossy — every pixel still reads back as the
    // same black — but it means the length of `pixels` is not stable across a
    // PNG round trip of a flat image, which is worth stating explicitly rather
    // than leaving to be discovered.
    if !writable("PNG") {
        return;
    }
    let source = image();
    let restored = Image::from_bytes(&source.write_bytes(Some("PNG")).unwrap()).expect("decode");
    let source_pixels = source.pixels(SampleDepth::Eight).unwrap();
    let restored_pixels = restored.pixels(SampleDepth::Eight).unwrap();

    assert!(
        restored_pixels.len() < source_pixels.len(),
        "a flat PNG was expected to be re-encoded more compactly"
    );
    assert!(
        restored_pixels.iter().all(|&v| v == 0),
        "but every sample must still be black"
    );
    assert_eq!(restored.size(), (W, H), "the geometry is unaffected");
}

#[test]
fn a_png_buffer_round_trips_through_the_blob_path() {
    let encoded = image().write_bytes(Some("PNG")).expect("encode");
    let restored = Image::from_bytes(&encoded).expect("decode");
    assert_eq!(restored.size(), (W, H));
}

// -- §11 the palette bug ---------------------------------------------------

#[test]
fn a_palette_image_without_transparency_reads_as_three_samples() {
    assert_eq!(
        mode_for_image(sys::PALETTE_TYPE, false).unwrap(),
        PixelMode::Palette
    );
    assert_eq!(PixelMode::Palette.channels(), 3);
}

#[test]
fn a_palette_image_with_transparency_reads_as_rgba() {
    // Regression: magick resolved *every* palette image to 3-sample RGB, so a
    // half-transparent pixel came back looking opaque and a fully transparent
    // one came back black. ImageMagick reports the transparency in the image
    // type, so that is the signal to key off.
    for image_type in [sys::PALETTE_ALPHA_TYPE, sys::PALETTE_BILEVEL_ALPHA_TYPE] {
        let mode = mode_for_image(image_type, true).expect("mode");
        assert_eq!(
            mode,
            PixelMode::Rgba,
            "{} must transfer the alpha channel",
            image_type_name(image_type)
        );
        assert_eq!(mode.channels(), 4);
    }
}

#[test]
fn real_transparency_survives_a_png_round_trip_intact() {
    if !writable("PNG") {
        return;
    }
    let source = Image::new(
        PixelMode::Rgba,
        (W, H),
        "rgba(10,20,30,0.5)",
        SampleDepth::Eight,
    )
    .expect("half-transparent image");
    let restored = Image::from_bytes(&source.write_bytes(Some("PNG")).unwrap()).expect("decode");
    assert_eq!(
        restored.pixels(SampleDepth::Eight).unwrap(),
        source.pixels(SampleDepth::Eight).unwrap(),
        "the alpha channel was lost in the PNG round trip"
    );
}

#[test]
fn the_image_type_table_covers_every_reported_type() {
    // `mode_for_image` must not reject a type ImageMagick can actually report,
    // or pixel access would fail on a file that opened perfectly well.
    for image_type in [
        sys::BILEVEL_TYPE,
        sys::GRAYSCALE_TYPE,
        sys::GRAYSCALE_ALPHA_TYPE,
        sys::PALETTE_TYPE,
        sys::PALETTE_ALPHA_TYPE,
        sys::PALETTE_BILEVEL_ALPHA_TYPE,
        sys::TRUECOLOR_TYPE,
        sys::TRUECOLOR_ALPHA_TYPE,
        sys::COLOR_SEPARATION_TYPE,
        sys::COLOR_SEPARATION_ALPHA_TYPE,
        sys::OPTIMIZE_TYPE,
    ] {
        assert!(
            mode_for_image(image_type, false).is_ok(),
            "type {} ({}) has no pixel mode",
            image_type,
            image_type_name(image_type)
        );
    }
}

#[test]
fn an_unsupported_image_type_is_an_operation_error_not_a_panic() {
    let error: Error = mode_for_image(9_999, false).expect_err("nonsense type");
    assert_eq!(error.kind(), ErrorKind::Operation);
}

// -- convenience -----------------------------------------------------------

/// Keeps the unused-import warning away while documenting the shape.
#[allow(dead_code)]
fn scratch_paths_are_absolute(scratch: &Scratch) -> Result<()> {
    assert!(scratch.outer().is_absolute());
    Ok(())
}
