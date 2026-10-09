//! Tests for `magik-core`'s pixel access and image construction.
//!
//! These complement `core.rs`: that file covers the Stage 01 surface, this one
//! covers pixel-level behaviour. Both are pure Rust — no Python involved —
//! which is exactly what the crate layering is meant to allow.

use magik_core::{ErrorKind, Image, PixelMode, SampleDepth};

// -- fixture helpers -------------------------------------------------------

const W: u32 = 8;
const H: u32 = 6;

fn rgb_image() -> Image {
    Image::new(PixelMode::Rgb, (W, H), "black", SampleDepth::Eight).expect("rgb")
}

fn gray_image() -> Image {
    Image::new(PixelMode::Grayscale, (W, H), "black", SampleDepth::Eight).expect("gray")
}

fn rgba_image() -> Image {
    Image::new(PixelMode::Rgba, (W, H), "black", SampleDepth::Eight).expect("rgba")
}

// -- Image.new -------------------------------------------------------------

#[test]
fn new_creates_the_requested_geometry() {
    for (mode, channels) in [
        (PixelMode::Grayscale, 1usize),
        (PixelMode::Rgb, 3),
        (PixelMode::Rgba, 4),
        (PixelMode::Cmyk, 4),
    ] {
        let image = Image::new(mode, (W, H), "black", SampleDepth::Eight).unwrap();
        assert_eq!(image.size(), (W, H));
        assert_eq!(image.pixel_mode().unwrap(), mode);
        assert_eq!(mode.channels(), channels);
    }
}

#[test]
fn new_reports_the_requested_mode() {
    assert_eq!(rgb_image().mode(), "RGB");
    assert_eq!(gray_image().mode(), "L");
    assert_eq!(rgba_image().mode(), "RGBA");
}

#[test]
fn new_fills_with_the_requested_colour() {
    let red = Image::new(PixelMode::Rgb, (2, 2), "(255,0,0)", SampleDepth::Eight).unwrap();
    assert_eq!(
        red.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![255, 0, 0]
    );
    assert_eq!(
        red.getpixel(1, 1, SampleDepth::Eight).unwrap(),
        vec![255, 0, 0]
    );

    let grey = Image::new(PixelMode::Grayscale, (2, 2), "128", SampleDepth::Eight).unwrap();
    assert_eq!(grey.getpixel(0, 0, SampleDepth::Eight).unwrap(), vec![128]);
}

#[test]
fn new_accepts_colour_strings() {
    let image = Image::new(PixelMode::Rgb, (2, 2), "#00ff00", SampleDepth::Eight).unwrap();
    assert_eq!(
        image.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![0, 255, 0]
    );
}

#[test]
fn new_rejects_zero_dimensions() {
    assert_eq!(
        Image::new(PixelMode::Rgb, (0, 10), "black", SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
    assert_eq!(
        Image::new(PixelMode::Rgb, (10, 0), "black", SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn new_rejects_an_unknown_mode() {
    assert_eq!(
        PixelMode::from_name("XYZ").unwrap_err().kind(),
        ErrorKind::Format
    );
}

#[test]
fn new_rejects_a_colour_with_the_wrong_channel_count() {
    let error = Image::new(PixelMode::Rgb, (2, 2), "(1,2)", SampleDepth::Eight).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
    assert!(error.message().contains("needs 3"));
}

// -- getpixel --------------------------------------------------------------

#[test]
fn getpixel_reads_every_mode() {
    assert_eq!(
        gray_image().getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![0]
    );
    assert_eq!(
        rgb_image()
            .getpixel(0, 0, SampleDepth::Eight)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        rgba_image()
            .getpixel(0, 0, SampleDepth::Eight)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn getpixel_is_fully_addressable() {
    let image = rgb_image();
    for y in 0..H as i64 {
        for x in 0..W as i64 {
            assert!(image.getpixel(x, y, SampleDepth::Eight).is_ok());
        }
    }
}

#[test]
fn getpixel_rejects_out_of_range_coordinates() {
    let image = rgb_image();
    for (x, y) in [(-1, 0), (0, -1), (W as i64, 0), (0, H as i64), (9999, 9999)] {
        let error = image.getpixel(x, y, SampleDepth::Eight).unwrap_err();
        assert_eq!(
            error.kind(),
            ErrorKind::Operation,
            "({x}, {y}) should be rejected"
        );
    }
}

#[test]
fn getpixel_accepts_the_last_pixel() {
    let image = rgb_image();
    assert!(image
        .getpixel(W as i64 - 1, H as i64 - 1, SampleDepth::Eight)
        .is_ok());
}

#[test]
fn getpixel_at_16_bit_depth_scales_up() {
    let image = Image::new(PixelMode::Grayscale, (2, 2), "255", SampleDepth::Eight).unwrap();
    let sixteen = image.getpixel(0, 0, SampleDepth::Sixteen).unwrap();
    assert_eq!(sixteen.len(), 1);
    // 8-bit 255 must map to 16-bit 65535, not stay at 255.
    assert_eq!(sixteen[0], 65535);
}

// -- putpixel --------------------------------------------------------------

#[test]
fn putpixel_writes_a_single_pixel() {
    let image = rgb_image();
    let edited = image
        .putpixel(2, 3, &[10, 20, 30], SampleDepth::Eight)
        .unwrap();
    assert_eq!(
        edited.getpixel(2, 3, SampleDepth::Eight).unwrap(),
        vec![10, 20, 30]
    );
}

#[test]
fn putpixel_leaves_neighbours_alone() {
    let image = rgb_image();
    let edited = image
        .putpixel(2, 3, &[10, 20, 30], SampleDepth::Eight)
        .unwrap();
    assert_eq!(
        edited.getpixel(1, 3, SampleDepth::Eight).unwrap(),
        vec![0, 0, 0]
    );
    assert_eq!(
        edited.getpixel(3, 3, SampleDepth::Eight).unwrap(),
        vec![0, 0, 0]
    );
}

#[test]
fn putpixel_does_not_mutate_the_receiver() {
    let image = rgb_image();
    let before = image.pixels(SampleDepth::Eight).unwrap();
    let edited = image
        .putpixel(0, 0, &[255, 255, 255], SampleDepth::Eight)
        .unwrap();
    assert_eq!(image.pixels(SampleDepth::Eight).unwrap(), before);
    assert_ne!(edited.pixels(SampleDepth::Eight).unwrap(), before);
}

#[test]
fn putpixel_rejects_a_wrong_channel_count() {
    let image = rgb_image();
    assert_eq!(
        image
            .putpixel(0, 0, &[1, 2], SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
    assert_eq!(
        image
            .putpixel(0, 0, &[1, 2, 3, 4], SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn putpixel_rejects_out_of_range_values() {
    let image = rgb_image();
    let error = image
        .putpixel(0, 0, &[256, 0, 0], SampleDepth::Eight)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
    assert!(error.message().contains("255"));
}

#[test]
fn putpixel_rejects_out_of_range_coordinates() {
    let image = rgb_image();
    assert_eq!(
        image
            .putpixel(W as i64, 0, &[1, 2, 3], SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn putpixel_round_trips_through_getpixel() {
    let image = rgba_image();
    let edited = image
        .putpixel(1, 1, &[11, 22, 33, 44], SampleDepth::Eight)
        .unwrap();
    assert_eq!(
        edited.getpixel(1, 1, SampleDepth::Eight).unwrap(),
        vec![11, 22, 33, 44]
    );
}

// -- pixels (bulk) ---------------------------------------------------------

#[test]
fn pixels_returns_the_expected_length() {
    for (mode, channels) in [
        (PixelMode::Grayscale, 1usize),
        (PixelMode::Rgb, 3),
        (PixelMode::Rgba, 4),
    ] {
        let image = Image::new(mode, (W, H), "black", SampleDepth::Eight).unwrap();
        assert_eq!(
            image.pixels(SampleDepth::Eight).unwrap().len(),
            (W * H) as usize * channels
        );
    }
}

#[test]
fn pixels_at_16_bit_depth_is_two_bytes_per_sample() {
    let image = Image::new(PixelMode::Rgb, (4, 4), "black", SampleDepth::Eight).unwrap();
    assert_eq!(
        image.pixels(SampleDepth::Sixteen).unwrap().len(),
        4 * 4 * 3 * 2
    );
}

#[test]
fn pixels_agrees_with_repeated_getpixel() {
    let image = rgb_image();
    let flat = image.pixels(SampleDepth::Eight).unwrap();
    for y in 0..H as i64 {
        for x in 0..W as i64 {
            let pixel = image.getpixel(x, y, SampleDepth::Eight).unwrap();
            let index = (y as usize * W as usize + x as usize) * 3;
            let slice: Vec<u8> = pixel.iter().map(|value| *value as u8).collect();
            assert_eq!(&flat[index..index + 3], slice.as_slice());
        }
    }
}

#[test]
fn pixels_survives_an_encode_decode_round_trip() {
    let image = Image::from_pixels(
        PixelMode::Rgb,
        (W, H),
        &checkerboard(W, H, 3),
        SampleDepth::Eight,
    )
    .unwrap();
    let encoded = image.write_bytes(Some("PNG")).unwrap();
    let decoded = Image::from_bytes(&encoded).unwrap();
    assert_eq!(
        decoded.pixels(SampleDepth::Eight).unwrap(),
        image.pixels(SampleDepth::Eight).unwrap()
    );
}

// -- putpixels (bulk region write) -----------------------------------------

#[test]
fn putpixels_writes_a_region() {
    let image = rgb_image();
    let region = vec![9u8; 2 * 2 * 3];
    let edited = image
        .putpixels((1, 1), (2, 2), &region, SampleDepth::Eight)
        .unwrap();
    assert_eq!(
        edited.getpixel(1, 1, SampleDepth::Eight).unwrap(),
        vec![9, 9, 9]
    );
    assert_eq!(
        edited.getpixel(2, 2, SampleDepth::Eight).unwrap(),
        vec![9, 9, 9]
    );
    // Outside the region is untouched.
    assert_eq!(
        edited.getpixel(3, 3, SampleDepth::Eight).unwrap(),
        vec![0, 0, 0]
    );
}

#[test]
fn putpixels_rejects_a_region_that_does_not_fit() {
    let image = rgb_image();
    let region = vec![0u8; 2 * 2 * 3];
    assert_eq!(
        image
            .putpixels((W as i64 - 1, 0), (2, 2), &region, SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn putpixels_rejects_a_wrong_length_buffer() {
    let image = rgb_image();
    assert_eq!(
        image
            .putpixels((0, 0), (2, 2), &[0u8; 5], SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn putpixels_does_not_mutate_the_receiver() {
    let image = rgb_image();
    let before = image.pixels(SampleDepth::Eight).unwrap();
    let _ = image
        .putpixels((0, 0), (2, 2), &[7u8; 12], SampleDepth::Eight)
        .unwrap();
    assert_eq!(image.pixels(SampleDepth::Eight).unwrap(), before);
}

// -- from_pixels -----------------------------------------------------------

#[test]
fn from_pixels_builds_from_exact_data() {
    let data = checkerboard(W, H, 3);
    let image = Image::from_pixels(PixelMode::Rgb, (W, H), &data, SampleDepth::Eight).unwrap();
    assert_eq!(image.size(), (W, H));
    assert_eq!(image.pixels(SampleDepth::Eight).unwrap(), data);
    assert_eq!(
        image.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![255, 0, 0]
    );
}

#[test]
fn from_pixels_rejects_a_short_buffer() {
    let error =
        Image::from_pixels(PixelMode::Rgb, (W, H), &[0u8; 5], SampleDepth::Eight).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
    assert!(error.message().contains("expected"));
}

#[test]
fn from_pixels_rejects_a_long_buffer() {
    let error =
        Image::from_pixels(PixelMode::Rgb, (W, H), &[0u8; 1000], SampleDepth::Eight).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn from_pixels_rejects_an_empty_buffer() {
    assert_eq!(
        Image::from_pixels(PixelMode::Rgb, (2, 2), &[], SampleDepth::Eight)
            .unwrap_err()
            .kind(),
        ErrorKind::Operation
    );
}

#[test]
fn from_pixels_rejects_zero_dimensions() {
    let error = Image::from_pixels(PixelMode::Rgb, (0, 4), &[], SampleDepth::Eight).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn from_pixels_at_16_bit_depth_round_trips_exactly() {
    let samples: Vec<u8> = (0..(W * H * 3))
        .flat_map(|i| (((i * 4000) % 65536) as u16).to_ne_bytes())
        .collect();
    let image = Image::from_pixels(PixelMode::Rgb, (W, H), &samples, SampleDepth::Sixteen).unwrap();
    assert_eq!(image.pixels(SampleDepth::Sixteen).unwrap(), samples);
}

// -- sizes -----------------------------------------------------------------

#[test]
fn works_at_several_sizes_including_odd_ones() {
    for (w, h) in [(1u32, 1u32), (1, 7), (7, 1), (3, 3), (16, 9), (33, 17)] {
        let data = checkerboard(w, h, 3);
        let image = Image::from_pixels(PixelMode::Rgb, (w, h), &data, SampleDepth::Eight).unwrap();
        assert_eq!(image.size(), (w, h));
        assert_eq!(image.pixels(SampleDepth::Eight).unwrap(), data);
        assert_eq!(
            image
                .getpixel(w as i64 - 1, h as i64 - 1, SampleDepth::Eight)
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn a_one_pixel_image_round_trips() {
    let image = Image::from_pixels(PixelMode::Rgb, (1, 1), &[1, 2, 3], SampleDepth::Eight).unwrap();
    assert_eq!(
        image.getpixel(0, 0, SampleDepth::Eight).unwrap(),
        vec![1, 2, 3]
    );
}

// -- interaction with existing operations ----------------------------------

#[test]
fn pixel_edits_survive_operations() {
    let image = rgb_image()
        .putpixel(0, 0, &[255, 0, 0], SampleDepth::Eight)
        .unwrap()
        .resize(4, 3)
        .unwrap()
        .grayscale()
        .unwrap();
    assert_eq!(image.size(), (4, 3));
    assert_eq!(image.getpixel(0, 0, SampleDepth::Eight).unwrap().len(), 1);
}

#[test]
fn newly_built_images_can_be_saved_and_reopened() {
    let image = Image::new(PixelMode::Rgb, (5, 5), "(0,0,255)", SampleDepth::Eight).unwrap();
    let bytes = image.write_bytes(Some("PNG")).unwrap();
    let reopened = Image::from_bytes(&bytes).unwrap();
    assert_eq!(reopened.size(), (5, 5));
    assert_eq!(
        reopened.getpixel(2, 2, SampleDepth::Eight).unwrap(),
        vec![0, 0, 255]
    );
}

// -- mode / depth parsing --------------------------------------------------

#[test]
fn mode_names_parse_case_insensitively() {
    assert_eq!(PixelMode::from_name("rgb").unwrap(), PixelMode::Rgb);
    assert_eq!(PixelMode::from_name("RGBA").unwrap(), PixelMode::Rgba);
    assert_eq!(PixelMode::from_name(" L ").unwrap(), PixelMode::Grayscale);
    assert_eq!(PixelMode::from_name("1").unwrap(), PixelMode::Bilevel);
    assert_eq!(PixelMode::from_name("cmyk").unwrap(), PixelMode::Cmyk);
}

#[test]
fn scalar_modes_report_scalar() {
    assert!(PixelMode::Grayscale.is_scalar());
    assert!(PixelMode::Bilevel.is_scalar());
    assert!(!PixelMode::Rgb.is_scalar());
    assert!(!PixelMode::Rgba.is_scalar());
}

#[test]
fn sample_depth_parses_and_rejects_others() {
    assert_eq!(SampleDepth::from_bits(8).unwrap(), SampleDepth::Eight);
    assert_eq!(SampleDepth::from_bits(16).unwrap(), SampleDepth::Sixteen);
    assert_eq!(
        SampleDepth::from_bits(32).unwrap_err().kind(),
        ErrorKind::Format
    );
}

#[test]
fn sample_depth_encode_preserves_endpoints() {
    assert_eq!(SampleDepth::Sixteen.encode(0), 0);
    assert_eq!(SampleDepth::Sixteen.encode(255), 65535);
    assert_eq!(SampleDepth::Eight.encode(255), 255);
}

// -- helper ----------------------------------------------------------------

/// Deterministic test pattern: red/green alternating pixels.
fn checkerboard(width: u32, height: u32, channels: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height) as usize * channels);
    for y in 0..height {
        for x in 0..width {
            if (x + y) % 2 == 0 {
                data.extend_from_slice(&[255, 0, 0]);
            } else {
                data.extend_from_slice(&[0, 255, 0]);
            }
        }
    }
    data
}
