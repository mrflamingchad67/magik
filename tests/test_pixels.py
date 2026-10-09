"""Stage 02: pixel access, image construction and bulk pixel transfers."""

from __future__ import annotations

import io

import pytest

import magik
from magik import Image


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def rgb():
    return Image.new("RGB", (8, 6), "black")


@pytest.fixture
def gray():
    return Image.new("L", (8, 6), "black")


@pytest.fixture
def rgba():
    return Image.new("RGBA", (8, 6), "black")


# ---------------------------------------------------------------------------
# Image.new
# ---------------------------------------------------------------------------


class TestNew:
    def test_creates_the_requested_geometry(self):
        for mode in ("1", "L", "RGB", "RGBA", "CMYK"):
            image = Image.new(mode, (12, 7), "black")
            assert image.size == (12, 7), mode

    def test_reports_the_requested_mode(self):
        for mode in ("1", "L", "RGB", "RGBA", "CMYK"):
            assert Image.new(mode, (4, 4), "black").mode == mode

    def test_default_colour_is_black(self):
        assert Image.new("RGB", (2, 2)).getpixel((0, 0)) == (0, 0, 0)

    @pytest.mark.parametrize(
        ("mode", "color", "expected"),
        [
            ("L", "white", 255),
            ("L", "black", 0),
            ("L", 128, 128),
            ("L", 0, 0),
            ("L", 255, 255),
        ],
    )
    def test_fills_with_a_number_or_name(self, mode, color, expected):
        assert Image.new(mode, (2, 2), color).getpixel((1, 1)) == expected

    @pytest.mark.parametrize(
        ("mode", "color", "expected"),
        [
            ("RGB", (255, 0, 0), (255, 0, 0)),
            ("RGB", [0, 128, 255], (0, 128, 255)),
            ("RGBA", (10, 20, 30, 40), (10, 20, 30, 40)),
        ],
    )
    def test_fills_with_a_sequence(self, mode, color, expected):
        assert Image.new(mode, (2, 2), color).getpixel((0, 0)) == expected

    def test_accepts_colour_strings(self):
        assert Image.new("RGB", (2, 2), "#00ff00").getpixel((0, 0)) == (0, 255, 0)
        assert Image.new("L", (2, 2), "gray(64)").getpixel((0, 0)) == 64

    def test_rgba_alpha_is_honoured(self):
        """ImageMagick reads rgba() alpha as a 0..1 fraction; magik converts."""
        assert Image.new("RGBA", (2, 2), (255, 0, 0, 0)).getpixel((0, 0))[3] == 0
        assert Image.new("RGBA", (2, 2), (255, 0, 0, 255)).getpixel((0, 0))[3] == 255
        half = Image.new("RGBA", (2, 2), (255, 0, 0, 128)).getpixel((0, 0))[3]
        assert abs(half - 128) <= 1, half

    def test_is_miff_backed_by_default(self):
        assert Image.new("RGB", (2, 2)).format == "MIFF"

    def test_new_images_can_be_saved(self, tmp_path):
        out = tmp_path / "new.png"
        Image.new("RGB", (5, 5), (0, 255, 0)).save(str(out))
        assert Image.open(out).getpixel((2, 2)) == (0, 255, 0)

    def test_new_images_can_be_resized(self):
        assert Image.new("RGB", (8, 8)).resize(4, 4).size == (4, 4)

    def test_one_pixel_image(self):
        assert Image.new("RGB", (1, 1), (1, 2, 3)).getpixel((0, 0)) == (1, 2, 3)

    @pytest.mark.parametrize("size", [(0, 4), (4, 0), (0, 0)])
    def test_rejects_zero_dimensions(self, size):
        with pytest.raises(magik.MagikOperationError):
            Image.new("RGB", size)

    def test_rejects_negative_dimensions(self):
        with pytest.raises(magik.MagikOperationError):
            Image.new("RGB", (-1, 4))

    def test_rejects_an_unknown_mode(self):
        with pytest.raises(magik.MagikFormatError) as info:
            Image.new("HSV", (2, 2))
        assert "supported modes" in str(info.value)

    def test_rejects_a_wrong_length_colour(self):
        with pytest.raises(magik.MagikOperationError):
            Image.new("RGB", (2, 2), (1, 2))

    def test_rejects_a_bad_size_type(self):
        with pytest.raises(TypeError):
            Image.new("RGB", "big")

    def test_rejects_an_invalid_colour_string(self):
        with pytest.raises(magik.MagikError):
            Image.new("RGB", (2, 2), "definitely-not-a-colour")

    def test_mode_listing_is_complete(self):
        assert magik.modes() == ["1", "L", "P", "RGB", "RGBA", "CMYK"]


# ---------------------------------------------------------------------------
# getpixel
# ---------------------------------------------------------------------------


class TestGetPixel:
    def test_scalar_modes_return_an_int(self, gray):
        value = gray.getpixel((0, 0))
        assert isinstance(value, int)
        assert not isinstance(value, bool)

    def test_multi_channel_modes_return_a_tuple(self, rgb, rgba):
        assert rgb.getpixel((0, 0)) == (0, 0, 0)
        assert rgba.getpixel((0, 0)) == (0, 0, 0, 255)

    def test_bilevel_returns_an_int(self):
        assert Image.new("1", (2, 2), "white").getpixel((0, 0)) == 255

    def test_every_pixel_is_addressable(self, rgb):
        for y in range(6):
            for x in range(8):
                assert rgb.getpixel((x, y)) == (0, 0, 0)

    def test_reads_the_value_written(self):
        image = Image.new("RGB", (4, 4), "black").putpixel((1, 2), (7, 8, 9))
        assert image.getpixel((1, 2)) == (7, 8, 9)

    def test_works_on_a_decoded_image(self):
        source = Image.open(io.BytesIO(_PNG()))
        assert len(source.getpixel((0, 0))) == 3

    @pytest.mark.parametrize(
        "xy", [(-1, 0), (0, -1), (8, 0), (0, 6), (99, 99), (-5, -5)]
    )
    def test_rejects_out_of_range_coordinates(self, rgb, xy):
        with pytest.raises(magik.MagikOperationError) as info:
            rgb.getpixel(xy)
        assert info.value.kind == "operation"

    def test_accepts_the_last_valid_pixel(self, rgb):
        assert rgb.getpixel((7, 5)) == (0, 0, 0)

    def test_rejects_a_bad_coordinate_type(self, rgb):
        with pytest.raises(TypeError):
            rgb.getpixel("nope")

    def test_rejects_a_wrong_length_coordinate(self, rgb):
        with pytest.raises(TypeError):
            rgb.getpixel((1, 2, 3))

    def test_at_16_bit_depth(self, gray):
        assert gray.getpixel((0, 0), depth=16) == 0
        white = Image.new("L", (2, 2), 255)
        assert white.getpixel((0, 0), depth=16) == 65535

    def test_rejects_an_unsupported_depth(self, rgb):
        with pytest.raises(magik.MagikFormatError):
            rgb.getpixel((0, 0), depth=32)


# ---------------------------------------------------------------------------
# putpixel
# ---------------------------------------------------------------------------


class TestPutPixel:
    def test_writes_the_pixel(self, rgb):
        assert rgb.putpixel((0, 0), (1, 2, 3)).getpixel((0, 0)) == (1, 2, 3)

    def test_returns_a_new_image(self, rgb):
        assert rgb.putpixel((0, 0), (1, 2, 3)) is not rgb

    def test_does_not_mutate_the_receiver(self, rgb):
        before = rgb.pixels()
        rgb.putpixel((0, 0), (255, 255, 255))
        assert rgb.pixels() == before
        assert rgb.getpixel((0, 0)) == (0, 0, 0)

    def test_leaves_neighbours_untouched(self, rgb):
        edited = rgb.putpixel((2, 2), (9, 9, 9))
        assert edited.getpixel((1, 2)) == (0, 0, 0)
        assert edited.getpixel((3, 2)) == (0, 0, 0)

    def test_scalar_modes_accept_an_int(self, gray):
        assert gray.putpixel((0, 0), 77).getpixel((0, 0)) == 77

    def test_accepts_a_list(self, rgb):
        assert rgb.putpixel((0, 0), [1, 2, 3]).getpixel((0, 0)) == (1, 2, 3)

    def test_accepts_an_rgba_tuple(self, rgba):
        assert rgba.putpixel((0, 0), (1, 2, 3, 4)).getpixel((0, 0)) == (1, 2, 3, 4)

    def test_alpha_is_preserved_exactly(self):
        image = Image.new("RGBA", (4, 4)).putpixel((1, 1), (9, 8, 7, 0))
        assert image.getpixel((1, 1)) == (9, 8, 7, 0)

    def test_chain_of_edits(self, rgb):
        edited = rgb.putpixel((0, 0), (1, 1, 1)).putpixel((1, 0), (2, 2, 2))
        assert edited.getpixel((0, 0)) == (1, 1, 1)
        assert edited.getpixel((1, 0)) == (2, 2, 2)

    @pytest.mark.parametrize("value", [(1, 2), (1, 2, 3, 4), ()])
    def test_rejects_a_wrong_channel_count(self, rgb, value):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixel((0, 0), value)

    def test_rejects_an_int_for_a_multi_channel_mode(self, rgb):
        with pytest.raises(TypeError):
            rgb.putpixel((0, 0), 5)

    def test_rejects_out_of_range_coordinates(self, rgb):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixel((99, 0), (1, 2, 3))

    def test_out_of_range_samples_are_clamped(self, rgb):
        edited = rgb.putpixel((0, 0), (300, -5, 0))
        assert edited.getpixel((0, 0)) == (255, 0, 0)

    def test_rejects_an_unsupported_depth(self, rgb):
        with pytest.raises(magik.MagikFormatError):
            rgb.putpixel((0, 0), (1, 2, 3), depth=7)


# ---------------------------------------------------------------------------
# pixels (bulk read)
# ---------------------------------------------------------------------------


class TestPixels:
    @pytest.mark.parametrize(
        ("mode", "channels"),
        [("L", 1), ("RGB", 3), ("RGBA", 4), ("CMYK", 4)],
    )
    def test_length_matches_geometry_and_mode(self, mode, channels):
        width, height = 8, 6
        assert len(Image.new(mode, (width, height)).pixels()) == width * height * channels

    def test_returns_bytes(self, rgb):
        assert isinstance(rgb.pixels(), bytes)

    def test_row_major_order(self):
        image = Image.new("RGB", (2, 2), "black")
        image = image.putpixel((1, 0), (1, 1, 1)).putpixel((0, 1), (2, 2, 2))
        assert image.pixels() == bytes([0, 0, 0, 1, 1, 1, 2, 2, 2, 0, 0, 0])

    def test_matches_repeated_getpixel(self, rgb):
        flat = rgb.pixels()
        for y in range(6):
            for x in range(8):
                pixel = rgb.getpixel((x, y))
                index = (y * 8 + x) * 3
                assert tuple(flat[index : index + 3]) == pixel

    def test_16_bit_is_two_bytes_per_sample(self):
        assert len(Image.new("RGB", (4, 4)).pixels(depth=16)) == 4 * 4 * 3 * 2

    def test_agrees_with_getpixel_at_16_bit(self, gray):
        image = Image.new("L", (4, 4), 255)
        flat = image.pixels(depth=16)
        assert flat[:2] == (65535).to_bytes(2, sys_byteorder())

    def test_one_pixel_image(self):
        assert Image.new("RGB", (1, 1), (1, 2, 3)).pixels() == bytes([1, 2, 3])

    def test_works_on_a_decoded_image(self):
        assert len(Image.open(io.BytesIO(_PNG())).pixels()) == _PNG_SIZE[0] * _PNG_SIZE[1] * 3

    def test_rejects_an_unsupported_depth(self, rgb):
        with pytest.raises(magik.MagikFormatError):
            rgb.pixels(depth=24)


# ---------------------------------------------------------------------------
# putpixels (bulk write)
# ---------------------------------------------------------------------------


class TestPutPixels:
    def test_writes_a_region(self, rgb):
        region = bytes([9] * 12)
        edited = rgb.putpixels((1, 1), (2, 2), region)
        assert edited.getpixel((1, 1)) == (9, 9, 9)
        assert edited.getpixel((2, 2)) == (9, 9, 9)

    def test_leaves_the_rest_alone(self, rgb):
        edited = rgb.putpixels((0, 0), (2, 2), bytes([9] * 12))
        assert edited.getpixel((3, 3)) == (0, 0, 0)

    def test_does_not_mutate_the_receiver(self, rgb):
        before = rgb.pixels()
        rgb.putpixels((0, 0), (2, 2), bytes([7] * 12))
        assert rgb.pixels() == before

    def test_whole_image_region(self, rgb):
        edited = rgb.putpixels((0, 0), (8, 6), bytes([5] * 8 * 6 * 3))
        assert edited.getpixel((7, 5)) == (5, 5, 5)

    def test_rejects_a_region_that_does_not_fit(self, rgb):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixels((6, 0), (4, 4), bytes(0))

    def test_rejects_a_wrong_length_buffer(self, rgb):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixels((0, 0), (2, 2), bytes(5))

    def test_rejects_a_zero_sized_region(self, rgb):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixels((0, 0), (0, 2), b"")

    def test_rejects_an_out_of_bounds_origin(self, rgb):
        with pytest.raises(magik.MagikOperationError):
            rgb.putpixels((99, 0), (1, 1), bytes(3))

    def test_16_bit_region(self, rgb):
        region = (1000).to_bytes(2, sys_byteorder()) * 3
        edited = rgb.putpixels((0, 0), (1, 1), region, depth=16)
        assert edited.getpixel((0, 0), depth=16) == (1000, 1000, 1000)


# ---------------------------------------------------------------------------
# from_pixels
# ---------------------------------------------------------------------------


class TestFromPixels:
    def test_builds_from_exact_data(self):
        data = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
        image = Image.from_pixels("RGB", (2, 2), data)
        assert image.size == (2, 2)
        assert image.pixels() == data

    def test_known_pixel_values(self):
        data = bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
        image = Image.from_pixels("RGB", (2, 2), data)
        assert image.getpixel((0, 0)) == (1, 2, 3)
        assert image.getpixel((1, 1)) == (10, 11, 12)

    def test_rgba_from_pixels(self):
        data = bytes([1, 2, 3, 4, 5, 6, 7, 8])
        image = Image.from_pixels("RGBA", (2, 1), data)
        assert image.getpixel((1, 0)) == (5, 6, 7, 8)

    def test_grayscale_from_pixels(self):
        image = Image.from_pixels("L", (3, 1), bytes([10, 20, 30]))
        assert [image.getpixel((x, 0)) for x in range(3)] == [10, 20, 30]

    def test_accepts_bytearray_and_memoryview(self):
        data = bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
        expected = Image.from_pixels("RGB", (2, 2), data).pixels()
        assert Image.from_pixels("RGB", (2, 2), bytearray(data)).pixels() == expected
        assert Image.from_pixels("RGB", (2, 2), memoryview(data)).pixels() == expected

    def test_16_bit_round_trip(self):
        values = [0, 1000, 20000, 65535, 32768, 1, 2, 3, 4, 5, 6, 7]
        data = b"".join(v.to_bytes(2, sys_byteorder()) for v in values)
        image = Image.from_pixels("RGB", (2, 2), data, depth=16)
        assert image.pixels(depth=16) == data

    @pytest.mark.parametrize("size", [(2, 2), (1, 1), (5, 3)])
    def test_accepts_the_exact_length(self, size):
        width, height = size
        data = bytes(width * height * 3)
        assert len(Image.from_pixels("RGB", size, data).pixels()) == len(data)

    @pytest.mark.parametrize("size", [(2, 2), (1, 1), (5, 3)])
    def test_rejects_a_wrong_length_buffer(self, size):
        with pytest.raises(magik.MagikOperationError):
            Image.from_pixels("RGB", size, bytes(7))

    def test_rejects_an_empty_buffer(self):
        with pytest.raises(magik.MagikOperationError):
            Image.from_pixels("RGB", (2, 2), b"")

    def test_rejects_an_unknown_mode(self):
        with pytest.raises(magik.MagikFormatError):
            Image.from_pixels("XYZ", (1, 1), bytes(3))

    def test_rejects_zero_dimensions(self):
        with pytest.raises(magik.MagikOperationError):
            Image.from_pixels("RGB", (0, 0), b"")

    def test_result_can_be_encoded_and_reopened(self):
        data = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
        image = Image.from_pixels("RGB", (2, 2), data)
        reopened = Image.open(io.BytesIO(image.to_bytes("PNG")))
        assert reopened.pixels() == data

    def test_result_is_immutable_under_edits(self):
        image = Image.from_pixels("RGB", (2, 2), bytes(12))
        before = image.pixels()
        image.putpixel((0, 0), (9, 9, 9))
        assert image.pixels() == before


# ---------------------------------------------------------------------------
# Round trips and interaction
# ---------------------------------------------------------------------------


class TestRoundTrip:
    @pytest.mark.parametrize("mode", ["1", "L", "RGB", "RGBA", "CMYK"])
    def test_png_round_trip_preserves_geometry(self, mode):
        """Every mode survives an encode/decode cycle with its size intact.

        Pixel *counts* are deliberately not compared here: ImageMagick's PNG
        encoder optimises, and a uniformly black RGB image legitimately comes
        back classified as grayscale. `test_non_uniform_rgb_round_trips_exactly`
        covers the lossless case with content that cannot be optimised away.
        """
        original = Image.new(mode, (8, 8), "black")
        reopened = Image.open(io.BytesIO(original.to_bytes("PNG")))
        assert reopened.size == original.size
        assert reopened.mode in magik.modes()

    @pytest.mark.parametrize("mode", ["1", "L", "RGB", "RGBA", "CMYK"])
    def test_png_round_trip_preserves_a_uniform_fill(self, mode):
        """Whatever type comes back, the image must still be one flat colour.

        The *value* is mode-dependent -- CMYK white is (0,0,0,0), not (255,...)
        -- so this asserts uniformity rather than a specific sample.
        """
        original = Image.new(mode, (8, 8), "white")
        reopened = Image.open(io.BytesIO(original.to_bytes("PNG")))
        flat = reopened.pixels()
        channels = len(flat) // (8 * 8)
        assert channels >= 1
        assert len(set(flat)) == 1, f"fill was not uniform: {sorted(set(flat))}"

    @pytest.mark.parametrize("mode", ["RGB", "RGBA", "L"])
    def test_lossless_round_trip_is_exact(self, mode):
        """MIFF is ImageMagick's native lossless format, so pixels must match."""
        channels = {"L": 1, "RGB": 3, "RGBA": 4}[mode]
        data = bytes((i * 37) % 256 for i in range(8 * 8 * channels))
        original = Image.from_pixels(mode, (8, 8), data)
        reopened = Image.open(io.BytesIO(original.to_bytes("MIFF")))
        assert reopened.mode == mode
        assert reopened.pixels() == data

    def test_png_encoding_may_reclassify_the_image_type(self):
        """Pinned-down ImageMagick behaviour, not a magik guarantee.

        The PNG encoder optimises: a highly repetitive RGBA image is small
        enough to be stored as a palette, so it comes back classified as `"P"`.
        magik's own formats, and non-repetitive content, are unaffected.
        """
        repetitive = bytes([9, 8, 7, 255] * (8 * 8))
        original = Image.from_pixels("RGBA", (8, 8), repetitive)
        reopened = Image.open(io.BytesIO(original.to_bytes("PNG")))
        assert reopened.mode == "P"
        # A palette image reads back as its resolved RGB, so the colour is intact.
        assert reopened.getpixel((0, 0))[:3] == (9, 8, 7)

    def test_non_uniform_rgb_survives_png(self):
        """Content PNG cannot quantise away round-trips exactly."""
        data = bytes((i * 37) % 256 for i in range(16 * 16 * 3))
        original = Image.from_pixels("RGB", (16, 16), data)
        reopened = Image.open(io.BytesIO(original.to_bytes("PNG")))
        assert reopened.pixels() == data

    def test_putpixel_survives_an_encode_cycle(self):
        image = Image.new("RGB", (4, 4), (10, 20, 30)).putpixel((2, 2), (200, 100, 50))
        reopened = Image.open(io.BytesIO(image.to_bytes("PNG")))
        assert reopened.getpixel((2, 2)) == (200, 100, 50)

    def test_pixels_survive_an_operation(self):
        image = Image.from_pixels("RGB", (8, 8), bytes([7] * 8 * 8 * 3))
        assert image.resize(4, 4).pixels() == bytes([7] * 4 * 4 * 3)

    def test_grayscale_then_pixel_access(self):
        image = Image.new("RGB", (4, 4), (255, 255, 255)).grayscale()
        assert image.pixel_mode == "L"
        assert isinstance(image.getpixel((0, 0)), int)


class TestImmutability:
    def test_no_pixel_operation_mutates_the_source(self, rgb):
        before = rgb.pixels()
        rgb.putpixel((0, 0), (255, 255, 255))
        rgb.putpixels((0, 0), (2, 2), bytes([1] * 12))
        assert rgb.pixels() == before

    def test_new_is_not_aliased(self):
        first = Image.new("RGB", (4, 4), "black")
        second = first.putpixel((0, 0), (255, 0, 0))
        assert first.getpixel((0, 0)) == (0, 0, 0)
        assert second.getpixel((0, 0)) == (255, 0, 0)


class TestThreading:
    def test_pixel_reads_are_safe_across_threads(self):
        from concurrent.futures import ThreadPoolExecutor

        image = Image.new("RGB", (32, 32), (3, 4, 5))
        with ThreadPoolExecutor(max_workers=8) as pool:
            values = list(pool.map(lambda _: image.getpixel((0, 0)), range(32)))
        assert values == [(3, 4, 5)] * 32

    def test_bulk_reads_are_safe_across_threads(self):
        from concurrent.futures import ThreadPoolExecutor

        image = Image.new("L", (32, 32), 9)
        expected = image.pixels()
        with ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(lambda _: image.pixels(), range(16)))
        assert all(result == expected for result in results)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def sys_byteorder() -> str:
    import sys

    return sys.byteorder


#: Geometry of the hand-built PNG below.
_PNG_SIZE = (4, 3)


def _PNG() -> bytes:
    """A tiny hand-built PNG, so decoding is not circular."""
    import struct
    import zlib

    width, height = _PNG_SIZE
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw += bytes(((x * 40) % 256, (y * 60) % 256, 90))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )