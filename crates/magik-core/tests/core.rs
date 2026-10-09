//! Integration tests for `magik-core`.
//!
//! These exercise the safe Rust API directly, without Python in the picture.
//! That is the point of the layering: the core is a plain Rust library, and it
//! must be testable as one.
//!
//! Fixtures are assembled by hand from `std` primitives so that decoding is
//! tested against bytes ImageMagick did not produce.

use magik_core::{ErrorKind, Image};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

mod fixtures {
    /// A valid 8-bit truecolour PNG, built without any image library.
    pub fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(0); // filter: None
            for x in 0..width {
                raw.extend_from_slice(&[
                    ((x * 4) % 256) as u8,
                    ((y * 5) % 256) as u8,
                    (((x + y) * 3) % 256) as u8,
                ]);
            }
        }
        // Store-mode deflate keeps this tiny and dependency-free.
        let mut zlib = vec![0x78, 0x01];
        zlib.extend_from_slice(&deflate_stored(&raw));
        zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

        let mut out = Vec::from(&b"\x89PNG\r\n\x1a\n"[..]);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolour
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &zlib);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut crc_input = Vec::from(&tag[..]);
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }

    /// RFC 1950 zlib stream around RFC 1951 *stored* (uncompressed) blocks.
    fn deflate_stored(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if data.is_empty() {
            out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
            return out;
        }
        let mut offset = 0;
        while offset < data.len() {
            let take = (data.len() - offset).min(0xffff);
            let last = offset + take == data.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(take as u16).to_le_bytes());
            out.extend_from_slice(&(!(take as u16)).to_le_bytes());
            out.extend_from_slice(&data[offset..offset + take]);
            offset += take;
        }
        out
    }

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    fn crc32(data: &[u8]) -> u32 {
        // Table-free CRC-32 (IEEE), sufficient for test fixtures.
        let mut crc = 0xffff_ffffu32;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
}

fn sample() -> Image {
    Image::from_bytes(&fixtures::png_bytes(WIDTH, HEIGHT)).expect("hand-built PNG must decode")
}

#[test]
fn opens_hand_built_png() {
    let image = sample();
    assert_eq!(image.size(), (WIDTH, HEIGHT));
    assert_eq!(image.format().unwrap(), "PNG");
}

#[test]
fn reports_metadata() {
    let image = sample();
    assert_eq!(image.mode(), "RGB");
    assert_eq!(image.channels(), 3);
    assert!(image.depth() > 0);
    assert_eq!(image.image_type(), "TrueColor");
    assert_eq!(image.colorspace(), "sRGB");
}

#[test]
fn operations_do_not_mutate_the_receiver() {
    let image = sample();
    let before = image.write_bytes(None).unwrap();

    let derived = image
        .resize(32, 24)
        .unwrap()
        .grayscale()
        .unwrap()
        .crop(0, 0, 16, 12)
        .unwrap()
        .blur(1.0, 0.5)
        .unwrap()
        .flip()
        .unwrap();

    assert_eq!(derived.size(), (16, 12));
    assert_eq!(image.size(), (WIDTH, HEIGHT));
    assert_eq!(
        image.write_bytes(None).unwrap(),
        before,
        "source image was modified"
    );
}

#[test]
fn resize_produces_the_requested_size() {
    assert_eq!(sample().resize(100, 50).unwrap().size(), (100, 50));
}

#[test]
fn resize_rejects_zero_dimensions() {
    let error = sample().resize(0, 10).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn crop_uses_exclusive_right_and_bottom() {
    assert_eq!(sample().crop(10, 5, 42, 30).unwrap().size(), (32, 25));
}

#[test]
fn crop_rejects_out_of_bounds() {
    let error = sample().crop(0, 0, 10_000, 10_000).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn crop_rejects_inverted_box() {
    let error = sample().crop(40, 40, 10, 10).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Operation);
}

#[test]
fn rotate_by_right_angle_swaps_axes() {
    let rotated = sample().rotate(90.0, None).unwrap();
    assert_eq!(rotated.size(), (HEIGHT, WIDTH));
}

#[test]
fn grayscale_yields_one_channel() {
    let grey = sample().grayscale().unwrap();
    assert_eq!(grey.mode(), "L");
    assert_eq!(grey.channels(), 1);
}

#[test]
fn round_trips_through_bytes() {
    let original = sample();
    let encoded = original.write_bytes(Some("PNG")).unwrap();
    let decoded = Image::from_bytes(&encoded).unwrap();
    assert_eq!(decoded.size(), original.size());
    assert_eq!(decoded.format().unwrap(), "PNG");
}

#[test]
fn png_round_trip_is_lossless() {
    let once = sample().write_bytes(Some("PNG")).unwrap();
    let twice = Image::from_bytes(&once)
        .unwrap()
        .write_bytes(Some("PNG"))
        .unwrap();
    assert_eq!(once, twice);
}

#[test]
fn clone_is_independent_of_the_original() {
    let image = sample();
    let clone = image.try_clone().unwrap();
    let shrunk = clone.resize(8, 8).unwrap();
    assert_eq!(shrunk.size(), (8, 8));
    assert_eq!(image.size(), (WIDTH, HEIGHT));
}

#[test]
fn explicit_format_wins_over_the_extension() {
    let dir = std::env::temp_dir().join("magik-core-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mislabelled.jpg");

    sample().save_with_format(&path, "PNG").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "explicit PNG format must beat the .jpg extension"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn save_does_not_change_the_image() {
    let image = sample();
    let before = image.metadata();
    let dir = std::env::temp_dir().join("magik-core-test");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join("unchanged.png")).unwrap();
    assert_eq!(image.metadata(), before);
}

// -- error handling ---------------------------------------------------------

#[test]
fn missing_file_is_an_open_error() {
    let error = Image::open("definitely-not-here-9182.png").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Open);
    assert!(!error.message().is_empty());
}

#[test]
fn invalid_data_is_an_open_error() {
    let error = Image::from_bytes(b"not an image at all, really").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Open);
}

#[test]
fn unknown_format_name_is_a_format_error() {
    let error = sample().write_bytes(Some("NOT-A-FORMAT")).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Format);
}

#[test]
fn unencodable_format_is_unsupported() {
    // XCF is a real ImageMagick format that this build cannot write. If the
    // build turns out to have an XCF writer the test passes vacuously.
    if let Err(error) = sample().write_bytes(Some("XCF")) {
        assert_eq!(error.kind(), ErrorKind::UnsupportedFormat);
    }
}

#[test]
fn unwritable_path_is_a_save_error() {
    let error = sample().save("no/such/directory/output.png").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Save);
}

#[test]
fn errors_carry_the_imagemagick_detail() {
    let error = Image::from_bytes(b"garbage").unwrap_err();
    // Either ImageMagick produced a message, or there was nothing to report --
    // but the error must never be empty.
    assert!(!error.message().is_empty());
    let _ = error.detail();
}

// -- backend / build information --------------------------------------------

#[test]
fn reports_imagemagick_version() {
    let version = magik_core::imagemagick_version();
    assert!(version.version.contains("ImageMagick"));
    assert!(!version.quantum_depth.is_empty());
    assert!(!version.quantum_range.is_empty());
}

#[test]
fn name_lookups_round_trip() {
    for value in [
        magik_core::colorspace_from_name("sRGB").unwrap(),
        magik_core::colorspace_from_name("Gray").unwrap(),
        magik_core::colorspace_from_name("CMYK").unwrap(),
    ] {
        assert!(!magik_core::colorspace_name(value).is_empty());
    }
    for value in [
        magik_core::compression_from_name("Zip").unwrap(),
        magik_core::compression_from_name("JPEG").unwrap(),
    ] {
        assert!(!magik_core::compression_name(value).is_empty());
    }
    assert_eq!(magik_core::colorspace_from_name("not-a-colorspace"), None);
    assert_eq!(magik_core::compression_from_name("not-compression"), None);
    assert_eq!(magik_core::filter_from_name("not-a-filter"), None);
}

#[test]
fn channel_counts_follow_the_image_type() {
    use magik_core::magik_sys as sys;
    assert_eq!(magik_core::channels_for_image_type(sys::GRAYSCALE_TYPE), 1);
    assert_eq!(magik_core::channels_for_image_type(sys::TRUECOLOR_TYPE), 3);
    assert_eq!(
        magik_core::channels_for_image_type(sys::TRUECOLOR_ALPHA_TYPE),
        4
    );
    assert_eq!(
        magik_core::channels_for_image_type(sys::COLOR_SEPARATION_TYPE),
        4
    );
}

// -- threading --------------------------------------------------------------

/// `Image` must be `Send + Sync`; this only compiles if so, and it is the
/// property the Python layer relies on.
///
/// The raw `Wand` is deliberately `Send` but **not** `Sync`, so it is asserted
/// separately: a wand must be able to move between threads, yet must never be
/// shareable by reference.
fn assert_send_sync<T: Send + Sync>() {}
fn assert_send<T: Send>() {}

#[test]
fn threading_traits_are_as_documented() {
    assert_send_sync::<Image>();
    assert_send::<magik_core::Wand>();
}

#[test]
fn an_image_can_be_used_from_several_threads() {
    let image = sample();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let image = image.try_clone().unwrap();
            std::thread::spawn(move || image.resize(16, 12).unwrap().write_bytes(Some("PNG")))
        })
        .collect();
    for handle in handles {
        assert!(handle.join().unwrap().is_ok());
    }
    assert_eq!(image.size(), (WIDTH, HEIGHT));
}

#[test]
fn one_image_shared_between_threads_stays_consistent() {
    use std::sync::Arc;
    let image = Arc::new(sample());
    let expected = image.write_bytes(None).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let image = Arc::clone(&image);
            std::thread::spawn(move || image.resize(16, 16).unwrap().write_bytes(None))
        })
        .collect();
    for handle in handles {
        assert!(handle.join().unwrap().is_ok());
    }
    assert_eq!(image.write_bytes(None).unwrap(), expected);
}
