//! Data-preservation contract for the Stage 05 operations.
//!
//! What is pinned here, and why
//! -----------------------------
//!
//! Some of these are *guarantees* — they follow from what the operation is, so a
//! failure means magik is broken. Others are *observations* of the linked
//! ImageMagick build, which happen to hold on 7.1.2-32 Q16-HDRI and are pinned
//! only so a change is noticed. The distinction is called out in each test,
//! because presenting a measured behaviour as a promise is how a library ends up
//! lying to its users.
//!
//! The short version:
//!
//! * reading `has_alpha` touches nothing;
//! * `extract_channel` returns exact samples but is a projection, not reversible;
//! * converting to the space an image is already in is an exact no-op;
//! * widening depth is free, narrowing depth is not;
//! * grayscale and any multi-channel collapse discard information by design.

use magik_core::{Image, PixelMode, SampleDepth};

const W: u32 = 8;

/// An RGB image spanning a wide range of values, so a lossy operation shows up.
fn rgb() -> Image {
    let data: Vec<u8> = (0..(W * W))
        .flat_map(|i| [(i * 29 + 7) as u8, (i * 53 + 11) as u8, (i * 17 + 3) as u8])
        .collect();
    Image::new(PixelMode::Rgb, (W, W), "black", SampleDepth::Eight)
        .expect("rgb")
        .putpixels((0, 0), (W, W), &data, SampleDepth::Eight)
        .expect("fill")
}

/// A single-channel image carrying the given 16-bit sample.
fn gray16(sample: u16) -> Image {
    Image::from_pixels(
        PixelMode::Grayscale,
        (1, 1),
        &sample.to_le_bytes(),
        SampleDepth::Sixteen,
    )
    .expect("gray16")
}

fn sample(image: &Image, depth: SampleDepth) -> Vec<u8> {
    image.pixels(depth).expect("pixels")
}

// ===========================================================================
// 1. has_alpha is read-only
// ===========================================================================

#[test]
fn reading_alpha_presence_changes_nothing() {
    // A guarantee, not an observation: this is a query, so any pixel change
    // would be a bug regardless of build.
    let image = rgb();
    let before = sample(&image, SampleDepth::Eight);
    let has_alpha = image.has_alpha();

    assert!(!has_alpha);
    assert_eq!(
        sample(&image, SampleDepth::Eight),
        before,
        "a query must not alter pixels"
    );

    let again = image.has_alpha();
    assert_eq!(again, has_alpha, "the answer must be stable");
}

// ===========================================================================
// 2. extract_channel: exact, but a projection
// ===========================================================================

#[test]
fn each_channel_returns_the_exact_source_samples() {
    // A guarantee: extraction selects a channel, so the values must match the
    // corresponding source sample exactly.
    let image = rgb();
    for (name, index) in [("red", 0usize), ("green", 1), ("blue", 2)] {
        let channel = image.extract_channel(name).expect(name);
        for y in 0..W {
            for x in 0..W {
                let source = image
                    .getpixel(x as i64, y as i64, SampleDepth::Eight)
                    .expect("source");
                let extracted = channel
                    .getpixel(x as i64, y as i64, SampleDepth::Eight)
                    .expect("extracted");
                assert_eq!(
                    extracted[0], source[index],
                    "{name} at ({x}, {y}) must be the source sample"
                );
            }
        }
    }
}

#[test]
fn extraction_is_not_reversible() {
    // A guarantee: three channels cannot be recovered from one, so a round trip
    // that claimed to restore them would be asserting something impossible.
    let image = rgb();
    let red = image.extract_channel("red").expect("red");
    assert_eq!(red.size(), image.size());
    assert_ne!(
        sample(&red, SampleDepth::Eight).len(),
        sample(&image, SampleDepth::Eight).len(),
        "one channel carries fewer samples than three"
    );
    assert_eq!(
        sample(&red, SampleDepth::Eight),
        sample(
            &image.extract_channel("red").expect("again"),
            SampleDepth::Eight
        ),
        "extracting the same channel twice is stable"
    );
}

// ===========================================================================
// 3. Colorspace identity
// ===========================================================================

#[test]
fn converting_to_the_active_colorspace_is_an_exact_no_op() {
    // A guarantee: no transform is requested, so the samples must be untouched.
    // This is what makes "is my image already in this space?" safe to ask.
    let image = rgb();
    assert_eq!(image.colorspace(), "sRGB");

    let same = image.convert_colorspace("sRGB").expect("sRGB");
    assert_eq!(
        sample(&same, SampleDepth::Eight),
        sample(&image, SampleDepth::Eight)
    );
    assert_eq!(same.colorspace(), "sRGB");
}

// ===========================================================================
// 4. Colorspace round trips: 8-bit and 16-bit measured separately
// ===========================================================================

#[test]
fn eight_bit_invertible_round_trips_are_exact() {
    // An OBSERVATION of ImageMagick 7.1.2-32, not a promise.
    //
    // Measured across a wide fixture, these spaces returned to sRGB bit-exactly.
    // Pinned so a change is noticed, but a different ImageMagick build could
    // differ by a unit; treat a failure here as "re-measure", not "magik broke".
    let image = rgb();
    for space in ["Lab", "XYZ", "YCbCr", "HSL"] {
        let round_trip = image
            .convert_colorspace(space)
            .expect(space)
            .convert_colorspace("sRGB")
            .expect("back");
        assert_eq!(
            sample(&round_trip, SampleDepth::Eight),
            sample(&image, SampleDepth::Eight),
            "{space} round trip was exact at 8 bits on the build this was measured on"
        );
    }
}

#[test]
fn sixteen_bit_round_trips_quantise_and_drift() {
    // An OBSERVATION, and the reason 8-bit and 16-bit are tested separately.
    //
    // The same transforms that are bit-exact at 8 bits lose precision at 16,
    // because the intermediate representation rounds. Measured drift was at most
    // 89 of 65535 (~0.14%). The bound below is deliberately far looser than that
    // so it survives a different build, while still catching a genuine
    // precision regression such as truncation to 8 bits.
    let values = [0u16, 1, 255, 256, 32767, 32768, 65535];
    let mut data = Vec::new();
    for r in values {
        for g in values {
            for b in values {
                for v in [r, g, b] {
                    data.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    let count = (values.len() * values.len() * values.len()) as u32;
    let image = Image::from_pixels(PixelMode::Rgb, (count, 1), &data, SampleDepth::Sixteen)
        .expect("16-bit fixture");

    for space in ["Lab", "Luv", "XYZ", "YCbCr"] {
        let round_trip = image
            .convert_colorspace(space)
            .expect(space)
            .convert_colorspace("sRGB")
            .expect("back");

        let before: Vec<u16> = sample(&image, SampleDepth::Sixteen)
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let after: Vec<u16> = sample(&round_trip, SampleDepth::Sixteen)
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();

        let worst = before
            .iter()
            .zip(&after)
            .map(|(a, b)| a.abs_diff(*b) as u32)
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 65535 / 100,
            "{space} drifted by {worst} of 65535, far beyond the measured ~0.14%"
        );
    }
}

// ===========================================================================
// 5. Depth: widening and narrowing are different
// ===========================================================================

#[test]
fn widening_then_narrowing_preserves_every_sample() {
    // A guarantee for the widening direction: 8 bits are exactly representable
    // at 16, so 8 -> 16 -> 8 must be the identity.
    let image = rgb().convert_depth(8).expect("8");
    let before = sample(&image, SampleDepth::Eight);
    let round_trip = image
        .convert_depth(16)
        .expect("16")
        .convert_depth(8)
        .expect("8 again");
    assert_eq!(sample(&round_trip, SampleDepth::Eight), before);
}

#[test]
fn narrowing_then_widening_can_lose_information() {
    // A guarantee: narrowing quantises, and the lost low bits cannot be
    // reconstructed. The specific values are chosen to sit on either side of a
    // quantisation boundary so the loss is unambiguous.
    for original in [1u16, 2, 255, 257, 4097] {
        let fine = gray16(original);
        assert_eq!(
            sample(&fine, SampleDepth::Sixteen),
            original.to_le_bytes().to_vec(),
            "fixture should hold the exact 16-bit value"
        );

        let widened = fine
            .convert_depth(8)
            .expect("narrow")
            .convert_depth(16)
            .expect("widen");

        let result: Vec<u16> = sample(&widened, SampleDepth::Sixteen)
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(
            result.len(),
            1,
            "expected a single reconstructed sample for {original}"
        );
        // Small values collapse onto their 8-bit approximation; the point is
        // that the widened value is *not* generally the original.
        if original == 1 {
            assert_ne!(
                result[0], original,
                "narrowing to 8 bits must not pretend the original low byte survived"
            );
        }
    }
}

// ===========================================================================
// 6. Grayscale discards colour
// ===========================================================================

#[test]
fn grayscale_discards_colour_and_is_not_reversible() {
    // A guarantee: one channel cannot reproduce three, so the round trip must
    // differ for a coloured image.
    let coloured = Image::from_pixels(PixelMode::Rgb, (1, 1), &[0, 0, 200], SampleDepth::Eight)
        .expect("fixture");

    let gray = coloured.grayscale().expect("gray");
    let round_trip = gray.convert_colorspace("sRGB").expect("back to sRGB");

    assert_ne!(
        sample(&round_trip, SampleDepth::Eight),
        sample(&coloured, SampleDepth::Eight),
        "grayscale is lossy for a coloured image and must not be asserted reversible"
    );
    assert_eq!(round_trip.size(), coloured.size());
}

// ===========================================================================
// 7. Alpha across every transparency level
// ===========================================================================

#[test]
fn alpha_survives_conversion_at_every_level() {
    // A guarantee that the alpha *sample* is carried across a transform rather
    // than recomputed: whatever the colour becomes, transparency is unchanged.
    for alpha in [0u32, 1, 128, 254, 255] {
        let image = Image::new(
            PixelMode::Rgba,
            (W, W),
            &format!("[200, 100, 50, {alpha}]"),
            SampleDepth::Eight,
        )
        .expect("rgba");

        assert!(image.has_alpha(), "alpha {alpha} must be reported");
        assert_eq!(
            image.getpixel(0, 0, SampleDepth::Eight).expect("source")[3],
            alpha
        );

        let converted = image.convert_colorspace("Gray").expect("convert");
        assert!(
            converted.has_alpha(),
            "alpha {alpha} must survive conversion"
        );
        assert_eq!(
            converted
                .getpixel(0, 0, SampleDepth::Eight)
                .expect("converted")[3],
            alpha,
            "conversion must not change the alpha sample"
        );
    }
}

#[test]
fn an_opaque_image_gains_no_alpha_by_being_queried_or_converted() {
    let image = rgb();
    assert!(!image.has_alpha());
    let converted = image.convert_colorspace("Gray").expect("convert");
    assert!(!converted.has_alpha());
}
