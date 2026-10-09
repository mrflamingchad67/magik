//! Stage 04 tests: the core operation set.
//!
//! Correctness is checked against **known pixel values** on small deterministic
//! fixtures rather than by eyeballing output. Where an operation is a pure
//! geometry transform the expected result is computed by hand, so a wrong
//! implementation fails loudly instead of drifting.
//!
//! Companion files: `core.rs` (Stage 01 surface), `pixel.rs` (Stage 02 pixel
//! engine), `io.rs` (Stage 03 image I/O).

use magik_core::{ErrorKind, Image, PixelMode, SampleDepth};

const W: u32 = 6;
const H: u32 = 4;

/// A deterministic image where every pixel is uniquely identifiable.
///
/// The value at `(x, y)` is derived only from its own coordinates, so a test can
/// name the pixel it expects after a transform without storing a reference copy.
fn marker(mode: PixelMode) -> Image {
    let channels = mode.channels() as u32;
    let mut data = Vec::with_capacity((W * H * channels) as usize);
    for y in 0..H {
        for x in 0..W {
            for channel in 0..channels {
                data.push((x * 37 + y * 11 + channel * 53) as u8);
            }
        }
    }
    let fill: Vec<&str> = match mode {
        PixelMode::Rgb => vec!["black", "black", "black"],
        PixelMode::Rgba => vec!["black", "black", "black", "black"],
        PixelMode::Grayscale => vec!["black"],
        _ => vec!["black"],
    };
    Image::new(mode, (W, H), fill[0], SampleDepth::Eight)
        .expect("new")
        .putpixels((0, 0), (W, H), &data, SampleDepth::Eight)
        .expect("fill")
}

/// The marker value for one coordinate and channel.
fn marker_at(x: u32, y: u32, channel: u32) -> u8 {
    (x * 37 + y * 11 + channel * 53) as u8
}

fn rgb() -> Image {
    marker(PixelMode::Rgb)
}

fn rgba() -> Image {
    marker(PixelMode::Rgba)
}

fn gray() -> Image {
    Image::new(PixelMode::Grayscale, (W, H), "black", SampleDepth::Eight).expect("gray")
}

/// The full flat pixel buffer, for exact equality comparisons.
fn buffer(image: &Image) -> Vec<u8> {
    image.pixels(SampleDepth::Eight).expect("pixels")
}

// ===========================================================================
// Resize
// ===========================================================================

#[test]
fn resize_produces_exactly_the_requested_geometry() {
    let image = rgb();
    for (w, h) in [(1, 1), (2, 3), (W, H), (12, 8), (64, 64), (300, 200)] {
        assert_eq!(
            image.resize(w, h).expect("resize").size(),
            (w, h),
            "{w}x{h}"
        );
    }
}

#[test]
fn resize_does_not_stretch_or_preserve_aspect_ratio_implicitly() {
    // magik resamples to an exact box, like Pillow's `resize`. It never infers
    // an aspect ratio, so this is a deliberate contract rather than an accident
    // that a future change might quietly alter.
    let square = gray().resize(10, 10).expect("resize");
    let wide = gray().resize(20, 10).expect("resize");
    assert_eq!(square.size(), (10, 10));
    assert_eq!(wide.size(), (20, 10));
}

#[test]
fn resize_is_deterministic() {
    let image = rgb();
    assert_eq!(
        buffer(&image.resize(9, 7).unwrap()),
        buffer(&image.resize(9, 7).unwrap()),
        "the same resize must produce identical bytes"
    );
}

#[test]
fn resize_rejects_zero_dimensions() {
    for (w, h) in [(0, 5), (5, 0), (0, 0)] {
        let error = rgb().resize(w, h).expect_err("zero dimension");
        assert_eq!(error.kind(), ErrorKind::Operation);
        assert!(
            error.to_string().contains("greater than 0"),
            "unhelpful message: {error}"
        );
    }
}

#[test]
fn resize_leaves_the_source_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let _ = image.resize(100, 100).expect("resize");
    assert_eq!(buffer(&image), before);
    assert_eq!(image.size(), (W, H));
}

#[test]
fn resize_survives_a_round_trip_through_tiny_dimensions() {
    // Degenerate but legal: collapsing to 1x1 and expanding back must not error.
    let image = rgb();
    assert_eq!(image.resize(1, 1).expect("collapse").size(), (1, 1));
    assert_eq!(
        image.resize(1, 1).unwrap().resize(W, H).unwrap().size(),
        (W, H)
    );
}

#[test]
fn an_explicit_filter_is_accepted_and_changes_the_result() {
    let image = rgb();
    let point = image
        .resize_with_filter(3, 2, magik_core::magick::filter_from_name("point").unwrap())
        .expect("point resize");
    let lanczos = image
        .resize_with_filter(
            3,
            2,
            magik_core::magick::filter_from_name("lanczos").unwrap(),
        )
        .expect("lanczos resize");
    assert_eq!(point.size(), lanczos.size());
    assert_ne!(
        buffer(&point),
        buffer(&lanczos),
        "different kernels must not produce identical output"
    );
}

// ===========================================================================
// Crop
// ===========================================================================

#[test]
fn crop_returns_the_requested_region_with_pillow_coordinates() {
    // `right`/`bottom` are exclusive, so the result is `right - left` wide.
    let image = rgb();
    assert_eq!(image.crop(0, 0, 3, 2).expect("crop").size(), (3, 2));
    assert_eq!(image.crop(2, 1, 5, 4).expect("crop").size(), (3, 3));
}

#[test]
fn crop_picks_up_exactly_the_expected_pixels() {
    let image = rgb();
    let cropped = image.crop(2, 1, 5, 4).expect("crop");
    for (dx, dy) in [(0, 0), (1, 0), (2, 0), (0, 1), (2, 2)] {
        let expected: Vec<u32> = (0..3)
            .map(|c| u32::from(marker_at(2 + dx, 1 + dy, c)))
            .collect();
        assert_eq!(
            cropped
                .getpixel(dx as i64, dy as i64, SampleDepth::Eight)
                .expect("getpixel"),
            expected,
            "crop offset ({dx}, {dy})"
        );
    }
}

#[test]
fn crop_supports_single_pixel_and_full_extent_boxes() {
    let image = rgb();
    assert_eq!(image.crop(0, 0, 1, 1).expect("1x1").size(), (1, 1));
    assert_eq!(
        image
            .crop(
                i64::from(W - 1),
                i64::from(H - 1),
                i64::from(W),
                i64::from(H)
            )
            .expect("corner")
            .size(),
        (1, 1)
    );
    assert_eq!(
        image
            .crop(0, 0, i64::from(W), i64::from(H))
            .expect("full")
            .size(),
        (W, H)
    );
}

#[test]
fn crop_rejects_degenerate_boxes() {
    for box_ in [(0, 0, 0, 4), (3, 0, 3, 4), (0, 0, 6, 0), (4, 4, 2, 2)] {
        let error = rgb()
            .crop(box_.0, box_.1, box_.2, box_.3)
            .expect_err("degenerate");
        assert_eq!(error.kind(), ErrorKind::Operation);
        assert!(
            error.to_string().contains("invalid crop box"),
            "unhelpful message: {error}"
        );
    }
}

#[test]
fn crop_rejects_out_of_bounds_regions_rather_than_padding() {
    // A deliberate divergence from Pillow, which silently pads out-of-bounds
    // requests with black. Padding needs extra canvas work magik does not do,
    // so an out-of-range box is reported instead of quietly invented.
    for box_ in [(0, 0, 7, 4), (0, 0, 6, 5), (0, 0, 100, 100)] {
        let error = rgb()
            .crop(box_.0, box_.1, box_.2, box_.3)
            .expect_err("out of bounds");
        assert!(
            error.to_string().contains("exceeds image bounds"),
            "expected a bounds error for {box_:?}, got: {error}"
        );
    }
}

#[test]
fn crop_rejects_negative_origins() {
    let error = rgb().crop(-1, 0, 3, 3).expect_err("negative origin");
    assert!(
        error.to_string().contains("must not be negative"),
        "{error}"
    );
}

#[test]
fn repeated_cropping_narrows_monotonically() {
    let image = rgb();
    let once = image.crop(0, 0, 4, 3).expect("crop");
    let twice = once.crop(1, 1, 3, 2).expect("crop");
    assert_eq!(twice.size(), (2, 1));
    assert_eq!(
        twice.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        image.getpixel(1, 1, SampleDepth::Eight).unwrap(),
        "a second crop must address the first crop's coordinates"
    );
    assert_eq!(image.size(), (W, H), "the original must be unchanged");
}

// ===========================================================================
// Rotate
// ===========================================================================

#[test]
fn right_angle_rotation_transposes_the_geometry() {
    let image = rgb();
    assert_eq!(image.rotate(0.0, None).expect("0").size(), (W, H));
    assert_eq!(image.rotate(180.0, None).expect("180").size(), (W, H));
    assert_eq!(image.rotate(360.0, None).expect("360").size(), (W, H));
    assert_eq!(image.rotate(90.0, None).expect("90").size(), (H, W));
    assert_eq!(image.rotate(270.0, None).expect("270").size(), (H, W));
    assert_eq!(image.rotate(-90.0, None).expect("-90").size(), (H, W));
}

#[test]
fn rotation_direction_is_counter_clockwise() {
    // magik matches Pillow, where a positive angle turns the image left. The
    // native ImageMagick call turns it right, so magik negates the angle; this
    // test pins the outcome so a sign error cannot slip through.
    //
    // A counter-clockwise quarter turn maps source (x, y) to new (y, W-1-x),
    // which carries the top-right corner to the top-left.
    let image = rgb();
    let ccw = image.rotate(90.0, None).expect("ccw");

    for x in 0..W {
        for y in 0..H {
            let expected: Vec<u32> = (0..3).map(|c| u32::from(marker_at(x, y, c))).collect();
            assert_eq!(
                ccw.getpixel(y as i64, (W - 1 - x) as i64, SampleDepth::Eight)
                    .unwrap(),
                expected,
                "source ({x}, {y}) should land at ({y}, {})",
                W - 1 - x
            );
        }
    }

    // The mirror-image rule identifies a clockwise turn, so assert they differ.
    let cw = image.rotate(-90.0, None).expect("cw");
    assert_ne!(buffer(&ccw), buffer(&cw));
}

#[test]
fn zero_and_full_turns_are_the_identity() {
    let image = rgb();
    assert_eq!(buffer(&image.rotate(0.0, None).unwrap()), buffer(&image));
    assert_eq!(buffer(&image.rotate(360.0, None).unwrap()), buffer(&image));
}

#[test]
fn four_quarter_turns_return_to_the_start() {
    let image = rgb();
    let round_trip = image
        .rotate(90.0, None)
        .unwrap()
        .rotate(90.0, None)
        .unwrap()
        .rotate(90.0, None)
        .unwrap()
        .rotate(90.0, None)
        .unwrap();
    assert_eq!(round_trip.size(), image.size());
    assert_eq!(buffer(&round_trip), buffer(&image));
}

#[test]
fn a_half_turn_is_a_point_reflection() {
    let image = rgb();
    let flipped = image.rotate(180.0, None).expect("180");
    for (x, y) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)] {
        let expected: Vec<u32> = (0..3)
            .map(|c| u32::from(marker_at(W - 1 - x, H - 1 - y, c)))
            .collect();
        assert_eq!(
            flipped
                .getpixel(x as i64, y as i64, SampleDepth::Eight)
                .unwrap(),
            expected,
            "corner ({x}, {y})"
        );
    }
}

#[test]
fn an_oblique_rotation_expands_the_canvas() {
    // Documented contract, verified against the backend rather than assumed:
    // `MagickRotateImage` grows the canvas so the rotated corners survive, so
    // the result is strictly larger than the source on both axes. An earlier
    // doc comment claimed the opposite.
    let rotated = rgb().rotate(45.0, None).expect("45");
    assert!(
        rotated.width() > W && rotated.height() > H,
        "expected an expanded canvas, got {:?} from {W}x{H}",
        rotated.size()
    );
}

#[test]
fn an_explicit_background_fills_the_exposed_corners() {
    let transparent = rgb().rotate(45.0, None).expect("45");
    let white = rgb().rotate(45.0, Some("white")).expect("45 on white");
    // Both are larger than the source, so (0,0) is an exposed corner.
    assert_eq!(
        white.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![255, 255, 255]
    );
    assert_ne!(
        transparent.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![255, 255, 255],
        "the default fill is transparent, not white"
    );
}

#[test]
fn rotation_rejects_non_finite_angles() {
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error = rgb().rotate(angle, None).expect_err("non-finite");
        assert_eq!(error.kind(), ErrorKind::Operation);
        assert!(error.to_string().contains("must be finite"), "{error}");
    }
}

#[test]
fn rotation_rejects_an_unparseable_background() {
    let error = rgb()
        .rotate(45.0, Some("definitely-not-a-colour"))
        .expect_err("bad colour");
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn rotation_leaves_the_source_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let _ = image.rotate(90.0, None).expect("rotate");
    assert_eq!(buffer(&image), before);
    assert_eq!(image.size(), (W, H));
}

// ===========================================================================
// Grayscale
// ===========================================================================

#[test]
fn grayscale_collapses_rgb_to_one_channel() {
    let gray = rgb().grayscale().expect("gray");
    assert_eq!(gray.size(), (W, H));
    assert_eq!(gray.pixel_mode().expect("mode"), PixelMode::Grayscale);
    assert_eq!(buffer(&gray).len(), (W * H) as usize);
}

#[test]
fn grayscale_preserves_alpha_and_keeps_the_image_readable() {
    // Alpha must survive: a see-through pixel stays see-through, it does not
    // become opaque.
    let gray = rgba().grayscale().expect("gray");
    let source = rgba().getpixel(0, 0, SampleDepth::Eight).unwrap();
    let converted = gray.getpixel(0, 0, SampleDepth::Eight).unwrap();

    assert_eq!(source.len(), 4);
    assert_eq!(converted.len(), 4, "alpha must still be transferred");
    assert_eq!(
        converted[3], source[3],
        "the alpha sample must be carried through unchanged"
    );
    assert_eq!(
        converted[0], converted[1],
        "the colour channels must be replicated"
    );
    assert_eq!(converted[1], converted[2]);
}

#[test]
fn grayscale_of_an_opaque_rgba_image_keeps_full_alpha() {
    let opaque = Image::new(
        PixelMode::Rgba,
        (W, H),
        "rgba(255,0,0,1.0)",
        SampleDepth::Eight,
    )
    .expect("opaque");
    let gray = opaque.grayscale().expect("gray");
    assert_eq!(gray.getpixel(0, 0, SampleDepth::Eight).unwrap()[3], 255);
}

#[test]
fn grayscale_is_idempotent() {
    let once = rgb().grayscale().expect("gray");
    let twice = once.grayscale().expect("gray again");
    assert_eq!(buffer(&twice), buffer(&once));
}

#[test]
fn grayscale_of_an_existing_grayscale_image_is_stable() {
    let image = gray();
    assert_eq!(
        image.grayscale().expect("gray").pixel_mode().unwrap(),
        PixelMode::Grayscale
    );
    assert_eq!(image.grayscale().unwrap().size(), image.size());
}

#[test]
fn grayscale_preserves_luminance_ordering() {
    // White must convert brighter than black, and a mid grey must land between
    // them. Exact values depend on the colorspace transform, so this asserts
    // monotonicity rather than a magic number.
    let ramp: Vec<u8> = (0..64).map(|v| v * 4).collect();
    let source = Image::new(PixelMode::Grayscale, (64, 1), "black", SampleDepth::Eight)
        .expect("ramp")
        .putpixels((0, 0), (64, 1), &ramp, SampleDepth::Eight)
        .expect("fill");
    let gray = buffer(&source.grayscale().expect("gray"));
    assert_eq!(gray.len(), 64);
    assert!(
        gray[0] < gray[32],
        "dark must stay darker than light: {gray:?}"
    );
    assert!(gray[63] < 255, "the ramp must not clip: {}", gray[63]);
}

#[test]
fn grayscale_leaves_the_source_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let _ = image.grayscale().expect("gray");
    assert_eq!(buffer(&image), before);
    assert_eq!(image.pixel_mode().unwrap(), PixelMode::Rgb);
}

// ===========================================================================
// Blur
// ===========================================================================

#[test]
fn blur_preserves_geometry() {
    let image = rgb();
    for (radius, sigma) in [(1.0, 0.5), (2.0, 1.0), (0.5, 0.25), (8.0, 4.0)] {
        assert_eq!(
            image.blur(radius, sigma).expect("blur").size(),
            (W, H),
            "blur({radius}, {sigma}) changed the size"
        );
    }
}

#[test]
fn zero_radius_and_sigma_is_the_identity() {
    let image = rgb();
    assert_eq!(buffer(&image.blur(0.0, 0.0).expect("blur")), buffer(&image));
}

#[test]
fn blur_smooths_the_image() {
    // A hard vertical edge must lose contrast under a blur. This catches a
    // backend call that silently did nothing.
    let mut data = Vec::new();
    for _ in 0..H {
        for x in 0..W {
            let v = if x < W / 2 { 0u8 } else { 255u8 };
            data.extend_from_slice(&[v, v, v]);
        }
    }
    let edge = Image::new(PixelMode::Rgb, (W, H), "black", SampleDepth::Eight)
        .expect("edge")
        .putpixels((0, 0), (W, H), &data, SampleDepth::Eight)
        .expect("fill");

    let blurred = buffer(&edge.blur(2.0, 1.0).expect("blur"));
    let original = buffer(&edge);

    // The two samples flanking the hard edge are the ones a blur must disturb:
    // the dark side must brighten and the bright side must dim.
    let middle_row = H / 2;
    let dark_side = (middle_row as usize) * (W as usize) * 3 + (W as usize / 2 - 1) * 3;
    let bright_side = (middle_row as usize) * (W as usize) * 3 + (W as usize / 2) * 3;

    assert!(
        blurred[dark_side] > original[dark_side],
        "the dark side of the edge should pick up light ({} -> {})",
        original[dark_side],
        blurred[dark_side]
    );
    assert!(
        blurred[bright_side] < original[bright_side],
        "the bright side of the edge should lose light ({} -> {})",
        original[bright_side],
        blurred[bright_side]
    );
}

#[test]
fn blur_applies_to_alpha_as_well_as_colour() {
    // Documented contract: every channel is blurred, matching Pillow. Two
    // checks pin it down from both directions - a uniform alpha field cannot
    // change, while a varying one must.
    let flat = Image::new(
        PixelMode::Rgba,
        (W, H),
        "rgba(10,20,30,0.75)",
        SampleDepth::Eight,
    )
    .expect("uniform alpha");
    let blurred_flat = flat.blur(2.0, 1.0).expect("blur");
    assert_eq!(
        blurred_flat.getpixel(0, 0, SampleDepth::Eight).unwrap()[3],
        flat.getpixel(0, 0, SampleDepth::Eight).unwrap()[3],
        "a uniform alpha field is unchanged by a blur"
    );
    assert_eq!(blurred_flat.pixel_mode().unwrap(), PixelMode::Rgba);

    // The marker fixture's alpha varies per pixel, so blurring must move it.
    let varied = rgba();
    let before: Vec<u32> = (0..W)
        .flat_map(|x| (0..H).map(move |y| (x, y)))
        .map(|(x, y)| {
            varied
                .getpixel(x as i64, y as i64, SampleDepth::Eight)
                .unwrap()[3]
        })
        .collect();
    let after: Vec<u32> = (0..W)
        .flat_map(|x| (0..H).map(move |y| (x, y)))
        .map(|(x, y)| {
            varied
                .blur(2.0, 1.0)
                .unwrap()
                .getpixel(x as i64, y as i64, SampleDepth::Eight)
                .unwrap()[3]
        })
        .collect();
    assert_ne!(
        before, after,
        "a varying alpha field must be blurred along with the colour"
    );
}

#[test]
fn blur_works_on_a_single_pixel_image() {
    let dot = Image::new(
        PixelMode::Rgb,
        (1, 1),
        "rgb(200,100,50)",
        SampleDepth::Eight,
    )
    .expect("dot");
    let blurred = dot.blur(2.0, 1.0).expect("blur");
    assert_eq!(blurred.size(), (1, 1));
    assert_eq!(
        blurred.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![200, 100, 50]
    );
}

#[test]
fn blur_rejects_invalid_parameters() {
    for (radius, sigma) in [
        (-1.0, 1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
        (f64::NAN, 1.0),
        (1.0, f64::NAN),
        (f64::INFINITY, 1.0),
        (1.0, f64::INFINITY),
    ] {
        let error = rgb().blur(radius, sigma).expect_err("invalid blur");
        assert_eq!(error.kind(), ErrorKind::Operation);
        assert!(
            error.to_string().contains("invalid blur parameters"),
            "{error}"
        );
    }
}

#[test]
fn blur_leaves_the_source_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let _ = image.blur(3.0, 1.5).expect("blur");
    assert_eq!(buffer(&image), before);
}

// ===========================================================================
// Flip and flop
// ===========================================================================

#[test]
fn flip_mirrors_vertically_and_flop_horizontally() {
    let image = rgb();
    assert_eq!(image.flip().expect("flip").size(), (W, H));
    assert_eq!(image.flop().expect("flop").size(), (W, H));

    // flip: (0,0) becomes the original bottom-left.
    let flipped = image.flip().expect("flip");
    let expected_flip: Vec<u32> = (0..3).map(|c| u32::from(marker_at(0, H - 1, c))).collect();
    assert_eq!(
        flipped.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        expected_flip
    );

    // flop: (0,0) becomes the original top-right.
    let flopped = image.flop().expect("flop");
    let expected_flop: Vec<u32> = (0..3).map(|c| u32::from(marker_at(W - 1, 0, c))).collect();
    assert_eq!(
        flopped.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        expected_flop
    );
}

#[test]
fn flip_and_flop_are_not_the_same_operation() {
    let image = rgb();
    assert_ne!(
        buffer(&image.flip().unwrap()),
        buffer(&image.flop().unwrap()),
        "an asymmetric image must distinguish the two"
    );
}

#[test]
fn applying_a_mirror_twice_is_the_identity() {
    let image = rgb();
    assert_eq!(
        buffer(&image.flip().unwrap().flip().unwrap()),
        buffer(&image)
    );
    assert_eq!(
        buffer(&image.flop().unwrap().flop().unwrap()),
        buffer(&image)
    );
}

#[test]
fn flipping_every_pixel_is_a_bilateral_reflection() {
    let image = rgb();
    let flipped = image.flip().expect("flip");
    for y in 0..H {
        for x in 0..W {
            let expected: Vec<u32> = (0..3)
                .map(|c| u32::from(marker_at(x, H - 1 - y, c)))
                .collect();
            assert_eq!(
                flipped
                    .getpixel(x as i64, y as i64, SampleDepth::Eight)
                    .unwrap(),
                expected,
                "pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn mirrors_preserve_transparent_pixels() {
    let image = rgba();
    let flipped = image.flip().expect("flip");
    for (x, y) in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)] {
        let source = image
            .getpixel(x as i64, y as i64, SampleDepth::Eight)
            .unwrap();
        let mirrored = flipped
            .getpixel(x as i64, (H - 1 - y) as i64, SampleDepth::Eight)
            .unwrap();
        assert_eq!(
            mirrored, source,
            "alpha must travel with the pixel at ({x}, {y})"
        );
    }
}

#[test]
fn mirroring_a_single_row_or_column_image_works() {
    let row = Image::new(PixelMode::Rgb, (4, 1), "black", SampleDepth::Eight).expect("row");
    assert_eq!(row.flip().expect("flip").size(), (4, 1));
    assert_eq!(row.flop().expect("flop").size(), (4, 1));

    let column = Image::new(PixelMode::Rgb, (1, 4), "black", SampleDepth::Eight).expect("col");
    assert_eq!(column.flip().expect("flip").size(), (1, 4));
    assert_eq!(column.flop().expect("flop").size(), (1, 4));
}

#[test]
fn mirroring_leaves_the_source_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let _ = image.flip().expect("flip");
    let _ = image.flop().expect("flop");
    assert_eq!(buffer(&image), before);
}

// ===========================================================================
// Cross-operation contracts
// ===========================================================================

#[test]
fn every_operation_leaves_its_input_untouched() {
    let image = rgb();
    let before = buffer(&image);
    let size = image.size();

    let _ = image.resize(3, 3).unwrap();
    let _ = image.crop(1, 1, 4, 3).unwrap();
    let _ = image.rotate(45.0, None).unwrap();
    let _ = image.grayscale().unwrap();
    let _ = image.blur(1.0, 0.5).unwrap();
    let _ = image.flip().unwrap();
    let _ = image.flop().unwrap();

    assert_eq!(buffer(&image), before, "an operation mutated its input");
    assert_eq!(image.size(), size);
}

#[test]
fn operations_chain_and_the_source_survives_the_whole_chain() {
    let source = rgb();
    let result = source
        .resize(8, 6)
        .unwrap()
        .crop(1, 1, 7, 5)
        .unwrap()
        .grayscale()
        .unwrap()
        .blur(1.0, 0.5)
        .unwrap()
        .flip()
        .unwrap();
    // crop is exclusive on the right/bottom: (1,1)-(7,5) of an 8x6 is 6x4.
    assert_eq!(result.size(), (6, 4));
    assert_eq!(source.size(), (W, H));
}

#[test]
fn operation_results_are_independent_of_each_other() {
    let image = rgb();
    let a = image.resize(4, 4).unwrap();
    let b = image.resize(9, 9).unwrap();
    assert_eq!(a.size(), (4, 4));
    assert_eq!(
        b.size(),
        (9, 9),
        "a later call must not resize an earlier result"
    );
}

#[test]
fn results_are_encodable_so_they_reach_the_io_layer() {
    let operated = rgb().grayscale().expect("gray");
    let encoded = operated.write_bytes(Some("PNG")).expect("encode");
    let reopened = Image::from_bytes(&encoded).expect("decode");
    assert_eq!(reopened.size(), (W, H));
}

#[test]
fn operations_behave_consistently_across_modes() {
    for mode in [PixelMode::Rgb, PixelMode::Rgba, PixelMode::Grayscale] {
        let image = marker(mode);
        let resized = image.resize(3, 3).expect("resize");
        let rotated = image.rotate(90.0, None).expect("rotate");
        let mirrored = image.flip().expect("flip");
        assert_eq!(resized.size(), (3, 3), "{mode:?} resize");
        assert_eq!(rotated.size(), (H, W), "{mode:?} rotate");
        assert_eq!(mirrored.size(), (W, H), "{mode:?} flip");
        assert_eq!(image.size(), (W, H), "{mode:?} was mutated");
    }
}
