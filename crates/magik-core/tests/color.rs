//! Stage 05 tests: colorspaces, channels, alpha and precision.
//!
//! Expectations are computed from measured ImageMagick behaviour rather than
//! assumed, and the values here are exact wherever the backend is exact.

use magik_core::{ErrorKind, Image, PixelMode, SampleDepth};

const W: u32 = 4;
const H: u32 = 4;

/// An RGB image whose channels all differ, so a mis-mapped channel is visible.
///
/// Every pixel is `(200, 100, 50)`; distinct per-channel values mean extracting
/// "green" cannot accidentally return red.
fn distinct_rgb() -> Image {
    let data: Vec<u8> = (0..(W * H)).flat_map(|_| [200u8, 100, 50]).collect();
    Image::new(PixelMode::Rgb, (W, H), "black", SampleDepth::Eight)
        .expect("rgb")
        .putpixels((0, 0), (W, H), &data, SampleDepth::Eight)
        .expect("fill")
}

/// An RGBA image with an exact alpha sample.
///
/// Uses the numeric-sequence form rather than an `rgba()` string on purpose:
/// ImageMagick parses `rgba()` alpha as a 0..1 *fraction*, so
/// `"rgba(200,100,50,128)"` is fully opaque. Stage 03 documented that edge, and
/// the sequence form is the one carrying absolute 0..255 samples.
fn rgba(alpha: u32) -> Image {
    Image::new(
        PixelMode::Rgba,
        (W, H),
        &format!("[200, 100, 50, {alpha}]"),
        SampleDepth::Eight,
    )
    .expect("rgba")
}

/// The single sample at the top-left, for single-channel results.
fn first(image: &Image) -> u32 {
    image.getpixel(0, 0, SampleDepth::Eight).expect("getpixel")[0]
}

// ===========================================================================
// Requirement A - conversion versus assignment
// ===========================================================================

/// sRGB luminance of `(0, 0, 200)`: `0.2126*R + 0.7152*G + 0.0722*B`.
const BLUE_200_LUMINANCE: u32 = 14;

fn blue_pixel() -> Image {
    Image::new(PixelMode::Rgb, (1, 1), "black", SampleDepth::Eight)
        .expect("rgb")
        .putpixels((0, 0), (1, 1), &[0, 0, 200], SampleDepth::Eight)
        .expect("fill")
}

#[test]
fn conversion_recomputes_pixel_values() {
    let image = blue_pixel();
    let gray = image.convert_colorspace("Gray").expect("convert");
    assert_eq!(
        first(&gray),
        BLUE_200_LUMINANCE,
        "a genuine conversion must produce the luminance, not a channel"
    );
}

#[test]
fn conversion_is_not_the_same_as_assignment() {
    // The distinction this documents is the point of Requirement A.
    //
    // Treating `(0, 0, 200)` *as* Gray leaves the samples alone, so reading the
    // first sample back yields the red channel, `0`. Converting recomputes
    // luminance, `14`. If these ever agree, `with_colorspace` has started
    // transforming and this module's documentation is wrong.
    let image = blue_pixel();
    let converted = image.convert_colorspace("Gray").expect("convert");
    let assigned = image.with_colorspace("Gray").expect("assign");

    assert_eq!(first(&converted), 14, "conversion computes luminance");
    assert_eq!(
        first(&assigned),
        0,
        "assignment leaves the samples untouched, so the first sample is red"
    );
    assert_ne!(
        converted.pixels(SampleDepth::Eight),
        assigned.pixels(SampleDepth::Eight)
    );
}

#[test]
fn grayscale_agrees_with_conversion_to_gray() {
    // `grayscale()` predates this stage and is documented as a transform; it
    // should be the special case of converting to Gray.
    let image = blue_pixel();
    assert_eq!(
        image.grayscale().expect("gray").pixels(SampleDepth::Eight),
        image
            .convert_colorspace("Gray")
            .expect("convert")
            .pixels(SampleDepth::Eight),
        "grayscale() and convert_colorspace('Gray') must agree"
    );
}

#[test]
fn conversion_preserves_dimensions() {
    let image = distinct_rgb();
    for name in ["Gray", "sRGB", "CMYK", "HSL", "Lab"] {
        let converted = image.convert_colorspace(name).expect("convert");
        assert_eq!(
            converted.size(),
            (W, H),
            "converting to {name} changed the size"
        );
    }
}

#[test]
fn conversion_preserves_alpha_when_the_destination_allows_it() {
    let converted = rgba(128).convert_colorspace("Gray").expect("convert");
    assert!(
        converted.has_alpha(),
        "alpha must survive a colorspace conversion"
    );
    let sample = converted
        .getpixel(0, 0, SampleDepth::Eight)
        .expect("getpixel");
    assert_eq!(
        sample[3], 128,
        "the alpha sample must be carried through unchanged, not recomputed"
    );
}

#[test]
fn converting_an_opaque_image_does_not_invent_alpha() {
    let converted = distinct_rgb().convert_colorspace("Gray").expect("convert");
    assert!(
        !converted.has_alpha(),
        "an opaque image must not gain alpha"
    );
}

#[test]
fn conversion_leaves_the_source_untouched() {
    let image = distinct_rgb();
    let before = image.pixels(SampleDepth::Eight).expect("pixels");
    let _ = image.convert_colorspace("Gray").expect("convert");
    let _ = image.convert_colorspace("CMYK").expect("convert");
    assert_eq!(image.pixels(SampleDepth::Eight).expect("pixels"), before);
    assert_eq!(
        image.colorspace(),
        "sRGB",
        "the source colorspace must be unchanged"
    );
}

#[test]
fn conversion_rejects_unknown_colorspaces() {
    for bad in ["NotAColorspace", "GrayAlpha", "", "   "] {
        let error = distinct_rgb().convert_colorspace(bad).expect_err("unknown");
        assert_eq!(
            error.kind(),
            ErrorKind::Format,
            "{bad:?} should be a format error"
        );
        assert!(error.to_string().contains("unknown colorspace"), "{error}");
    }
}

#[test]
fn conversion_accepts_the_same_names_as_the_assignment_api() {
    // Both paths must resolve names through one table, so a name that works
    // for one works for the other.
    for name in ["Gray", "grey", "l", "sRGB", "CMYK"] {
        assert!(
            distinct_rgb().convert_colorspace(name).is_ok(),
            "{name:?} should be accepted"
        );
    }
}

#[test]
fn a_gray_round_trip_is_lossy_and_does_not_claim_otherwise() {
    // Collapsing three channels to one and back cannot restore the original.
    // The test asserts the *loss* so nobody later "fixes" it by asserting
    // equality that the backend never promised.
    let image = blue_pixel();
    let round_trip = image
        .convert_colorspace("Gray")
        .expect("to gray")
        .convert_colorspace("sRGB")
        .expect("back");
    assert_ne!(
        round_trip.pixels(SampleDepth::Eight),
        image.pixels(SampleDepth::Eight),
        "a round trip through Gray is lossy and must not be asserted equal"
    );
}

#[test]
fn conversion_is_deterministic() {
    let image = distinct_rgb();
    assert_eq!(
        image
            .convert_colorspace("Gray")
            .expect("a")
            .pixels(SampleDepth::Eight),
        image
            .convert_colorspace("Gray")
            .expect("b")
            .pixels(SampleDepth::Eight)
    );
}

// ===========================================================================
// Requirement B - channel extraction
// ===========================================================================

#[test]
fn extracting_rgb_channels_returns_their_own_values() {
    let image = distinct_rgb();
    for (name, expected) in [("red", 200u32), ("green", 100), ("blue", 50)] {
        let channel = image.extract_channel(name).expect(name);
        assert_eq!(first(&channel), expected, "{name} channel value");
        assert_eq!(channel.size(), (W, H), "{name} must keep the geometry");
        assert_eq!(channel.pixel_mode().expect("mode"), PixelMode::Grayscale);
    }
}

#[test]
fn extracting_alpha_returns_the_alpha_samples() {
    let channel = rgba(128).extract_channel("alpha").expect("alpha");
    assert_eq!(first(&channel), 128);
    assert!(
        !channel.has_alpha(),
        "the extracted alpha is opaque data, not an alpha channel"
    );
}

#[test]
fn extracting_a_channel_does_not_modify_the_source() {
    let image = distinct_rgb();
    let before = image.pixels(SampleDepth::Eight).expect("pixels");
    for name in ["red", "green", "blue"] {
        let _ = image.extract_channel(name).expect(name);
    }
    assert_eq!(image.pixels(SampleDepth::Eight).expect("pixels"), before);
    assert_eq!(image.size(), (W, H));
    assert!(
        !image.has_alpha(),
        "extraction must not give the source an alpha channel"
    );
}

#[test]
fn extracting_alpha_from_an_opaque_image_is_an_error() {
    // Documented contract: a descriptive error rather than a fabricated image.
    let error = distinct_rgb()
        .extract_channel("alpha")
        .expect_err("no alpha");
    assert_eq!(error.kind(), ErrorKind::Operation);
    let message = error.to_string();
    assert!(message.contains("alpha"), "{message}");
    assert!(
        message.contains("red") || message.contains("no separable"),
        "the error should say what is available: {message}"
    );
}

#[test]
fn extracting_colour_from_a_grayscale_image_is_an_error() {
    // Guards the positional-alias trap: `red` and `gray` are both "the first
    // channel" in ImageMagick, so without validation this would silently
    // return the luminance as though it were red.
    let gray = Image::new(PixelMode::Grayscale, (W, H), "black", SampleDepth::Eight).expect("gray");
    for name in ["red", "green", "blue"] {
        let error = gray.extract_channel(name).expect_err(name);
        assert_eq!(error.kind(), ErrorKind::Operation, "{name} on a gray image");
    }
}

#[test]
fn extracting_cmyk_channels_returns_each_component() {
    let cmyk = Image::new(
        PixelMode::Cmyk,
        (W, H),
        &format!(
            "cmyk({},{},{},{})",
            10 * 255 / 100,
            20 * 255 / 100,
            30 * 255 / 100,
            40 * 255 / 100
        ),
        SampleDepth::Eight,
    )
    .expect("cmyk");
    // Only assert that each channel is extractable and single-channel; the exact
    // CMYK sample mapping is ImageMagick's, and pinning it here would encode a
    // detail of the backend rather than magik's contract.
    for name in ["cyan", "magenta", "yellow", "black"] {
        let channel = cmyk.extract_channel(name).expect(name);
        assert_eq!(channel.size(), (W, H));
        assert_eq!(channel.pixel_mode().expect("mode"), PixelMode::Grayscale);
    }
    let error = cmyk.extract_channel("red").expect_err("red on cmyk");
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn extracting_rejects_unknown_channel_names() {
    for bad in ["notachannel", "", "   ", "luma", "gray"] {
        let error = distinct_rgb().extract_channel(bad).expect_err("unknown");
        assert_eq!(
            error.kind(),
            ErrorKind::Format,
            "{bad:?} should be a format error"
        );
        assert!(error.to_string().contains("unknown channel"), "{error}");
    }
}

#[test]
fn channel_names_are_case_insensitive() {
    let image = distinct_rgb();
    assert_eq!(
        first(&image.extract_channel("RED").expect("upper")),
        first(&image.extract_channel("red").expect("lower"))
    );
}

#[test]
fn extraction_is_repeatable() {
    let image = distinct_rgb();
    assert_eq!(
        image
            .extract_channel("green")
            .expect("a")
            .pixels(SampleDepth::Eight),
        image
            .extract_channel("green")
            .expect("b")
            .pixels(SampleDepth::Eight)
    );
}

#[test]
fn extracted_channels_survive_encoding() {
    let channel = distinct_rgb().extract_channel("red").expect("red");
    let encoded = channel.write_bytes(Some("PNG")).expect("encode");
    let reopened = Image::from_bytes(&encoded).expect("decode");
    assert_eq!(reopened.size(), (W, H));
}

// ===========================================================================
// Requirement C - alpha introspection
// ===========================================================================

#[test]
fn alpha_presence_matches_the_images_actual_state() {
    let cases: &[(Image, bool, &str)] = &[
        (distinct_rgb(), false, "RGB"),
        (rgba(255), true, "RGBA fully opaque"),
        (rgba(128), true, "RGBA partially transparent"),
        (rgba(0), true, "RGBA fully transparent"),
        (
            Image::new(PixelMode::Grayscale, (W, H), "black", SampleDepth::Eight).expect("L"),
            false,
            "L",
        ),
        (
            Image::new(PixelMode::Cmyk, (W, H), "black", SampleDepth::Eight).expect("CMYK"),
            false,
            "CMYK",
        ),
    ];
    for (image, expected, label) in cases {
        assert_eq!(image.has_alpha(), *expected, "{label}");
    }
}

#[test]
fn alpha_cannot_be_inferred_from_the_channel_count() {
    // CMYK carries four channels and no alpha. Any implementation that guesses
    // from `channels` would report `true` here, which is why this is pinned.
    let cmyk = Image::new(PixelMode::Cmyk, (W, H), "black", SampleDepth::Eight).expect("cmyk");
    assert_eq!(cmyk.channels(), 4);
    assert!(!cmyk.has_alpha(), "four channels is not evidence of alpha");
}

#[test]
fn alpha_survives_an_encode_and_reopen() {
    let encoded = rgba(128).write_bytes(Some("MIFF")).expect("encode");
    assert!(Image::from_bytes(&encoded).expect("decode").has_alpha());
}

// ===========================================================================
// Requirement D - depth and precision
// ===========================================================================

#[test]
fn depth_conversion_reports_the_new_depth() {
    let image = distinct_rgb();
    assert_eq!(image.convert_depth(8).expect("8").depth(), 8);
    assert_eq!(image.convert_depth(16).expect("16").depth(), 16);
}

#[test]
fn reducing_depth_discards_precision_that_cannot_be_recovered() {
    // The central honesty test of this stage.
    //
    // A 16-bit sample of `1` is `0x0001`. At 8 bits it rounds to `1`, and
    // widening that back to 16 yields `0x0101`, not `0x0001`. The conversion is
    // therefore lossy in the only direction that matters, and this test fails if
    // anyone later claims otherwise.
    let fine = Image::from_pixels(
        PixelMode::Grayscale,
        (1, 1),
        &[0x00, 0x01],
        SampleDepth::Sixteen,
    )
    .expect("16-bit source");
    assert_eq!(
        fine.pixels(SampleDepth::Sixteen).expect("16-bit"),
        vec![0x00, 0x01]
    );

    let widened = fine
        .convert_depth(8)
        .expect("to 8")
        .convert_depth(16)
        .expect("to 16");
    assert_eq!(
        widened.pixels(SampleDepth::Sixteen).expect("16-bit"),
        vec![0x01, 0x01],
        "widening a quantised sample must not imply the original detail survived"
    );
}

#[test]
fn widening_then_narrowing_again_is_stable() {
    // Going up must not invent or destroy colour, so 8 -> 16 -> 8 is the
    // identity. This is the harmless direction, and it is worth pinning so the
    // lossy case above cannot be "fixed" by breaking this one.
    let image = Image::new(PixelMode::Grayscale, (W, H), "black", SampleDepth::Eight)
        .expect("gray")
        .putpixels(
            (0, 0),
            (W, H),
            &(0..(W * H))
                .map(|i| (i as u8).wrapping_mul(17))
                .collect::<Vec<u8>>(),
            SampleDepth::Eight,
        )
        .expect("fill");
    let before = image
        .convert_depth(8)
        .expect("8")
        .pixels(SampleDepth::Eight)
        .expect("p");
    let after = image
        .convert_depth(8)
        .expect("8")
        .convert_depth(16)
        .expect("16")
        .convert_depth(8)
        .expect("8 again")
        .pixels(SampleDepth::Eight)
        .expect("p");
    assert_eq!(before, after, "8 -> 16 -> 8 must not change the samples");
}

#[test]
fn depth_conversion_rejects_unsupported_widths() {
    for bad in [0u32, 1, 7, 24, 32, 64] {
        let error = distinct_rgb().convert_depth(bad).expect_err("unsupported");
        assert_eq!(
            error.kind(),
            ErrorKind::Format,
            "{bad} bits should be rejected"
        );
        assert!(error.to_string().contains("8 and 16"), "{error}");
    }
}

#[test]
fn depth_conversion_leaves_the_source_untouched() {
    let image = distinct_rgb();
    let before = image.pixels(SampleDepth::Eight).expect("pixels");
    let _ = image.convert_depth(8).expect("8");
    let _ = image.convert_depth(16).expect("16");
    assert_eq!(image.pixels(SampleDepth::Eight).expect("pixels"), before);
}

#[test]
fn pixel_transfer_depth_is_independent_of_image_depth() {
    // The two depths are separate concepts and must not be conflated: the
    // `depth=` argument chooses the transfer width, `depth()` reports the
    // image's own.
    let image = Image::from_pixels(
        PixelMode::Rgb,
        (W, H),
        &vec![0u8; (W * H * 3 * 2) as usize],
        SampleDepth::Sixteen,
    )
    .expect("16-bit source");

    assert_eq!(
        image.pixels(SampleDepth::Eight).expect("8-bit").len(),
        (W * H * 3) as usize
    );
    assert_eq!(
        image.pixels(SampleDepth::Sixteen).expect("16-bit").len(),
        (W * H * 3 * 2) as usize
    );
}

#[test]
fn depth_conversion_result_can_be_encoded_and_reopened() {
    let converted = distinct_rgb().convert_depth(8).expect("8");
    let encoded = converted.write_bytes(Some("MIFF")).expect("encode");
    let reopened = Image::from_bytes(&encoded).expect("decode");
    assert_eq!(reopened.size(), (W, H));
    assert_eq!(
        reopened.pixels(SampleDepth::Eight).expect("pixels"),
        converted.pixels(SampleDepth::Eight).expect("pixels")
    );
}

// ===========================================================================
// Cross-cutting contracts
// ===========================================================================

#[test]
fn every_new_operation_returns_an_immutable_result() {
    let image = distinct_rgb();
    let before = image.pixels(SampleDepth::Eight).expect("pixels");
    let results = [
        image.convert_colorspace("Gray").expect("convert"),
        image.extract_channel("red").expect("channel"),
        image.convert_depth(16).expect("depth"),
        image.grayscale().expect("gray"),
    ];
    for result in &results {
        assert_eq!(result.size(), (W, H));
        assert_eq!(image.pixels(SampleDepth::Eight).expect("pixels"), before);
    }
}

#[test]
fn results_interoperate_with_existing_apis() {
    let result = distinct_rgb().convert_colorspace("Gray").expect("convert");
    assert!(result.getpixel(0, 0, SampleDepth::Eight).is_ok());
    assert!(!result
        .pixels(SampleDepth::Eight)
        .expect("pixels")
        .is_empty());
    assert!(!result.metadata().format.is_empty());
    assert!(result.write_bytes(Some("PNG")).is_ok());
}
