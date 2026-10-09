"""Metadata: width, height, format, mode, channels, depth and friends."""

from __future__ import annotations

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, WIDTH, Fixtures


class TestDimensions:
    def test_width_and_height(self, image: Image) -> None:
        assert image.width == WIDTH
        assert image.height == HEIGHT

    def test_size_tuple_matches(self, image: Image) -> None:
        assert image.size == (image.width, image.height) == (WIDTH, HEIGHT)

    def test_size_changes_after_resize(self, image: Image) -> None:
        assert image.resize(20, 10).size == (20, 10)


class TestFormat:
    def test_png(self, image: Image) -> None:
        assert image.format == "PNG"

    def test_jpeg(self, fixtures: Fixtures, need_format) -> None:
        need_format("JPEG", fixtures)
        assert fixtures.image("JPEG").format == "JPEG"

    def test_format_is_uppercase(self, fixtures: Fixtures, need_format) -> None:
        need_format("GIF", fixtures)
        assert fixtures.image("GIF").format == fixtures.image("GIF").format.upper()


class TestMode:
    """`mode` is a documented projection of ImageMagick's image type."""

    def test_truecolor_png_is_rgb(self, image: Image) -> None:
        assert image.mode == "RGB"
        assert image.image_type == "TrueColor"

    def test_grayscale_is_l(self, image: Image) -> None:
        grey = image.grayscale()
        assert grey.mode == "L"
        assert grey.image_type == "Grayscale"

    def test_mode_is_one_of_the_documented_values(self, image: Image) -> None:
        assert image.mode in {"1", "L", "LA", "P", "RGB", "RGBA", "CMYK"}

    def test_gray_and_rgb_channel_counts_differ(self, image: Image) -> None:
        assert image.channels == 3
        assert image.grayscale().channels == 1

    @pytest.mark.parametrize(
        ("mode", "expected"),
        [("L", "L"), ("RGB", "RGB"), ("RGBA", "RGBA"), ("1", "1"), ("CMYK", "CMYK")],
    )
    def test_each_mode_reports_its_own_name(self, mode: str, expected: str) -> None:
        """Regression: RGBA images used to be reported as "RGB".

        `mode` is a projection of ImageMagick's `ImageType`, and the
        `TRUECOLOR_ALPHA_TYPE` arm was missing, so every alpha image fell through
        to the "RGB" catch-all.
        """
        assert Image.new(mode, (2, 2)).mode == expected

    def test_rgba_images_really_carry_alpha(self) -> None:
        rgba = Image.new("RGBA", (2, 2), (1, 2, 3, 4))
        assert rgba.mode == "RGBA"
        assert rgba.channels == 4
        assert rgba.getpixel((0, 0)) == (1, 2, 3, 4)

    def test_alpha_survives_an_encode_cycle(self) -> None:
        import io

        rgba = Image.new("RGBA", (4, 4), (9, 8, 7, 6))
        reopened = Image.open(io.BytesIO(rgba.to_bytes("MIFF")))
        assert reopened.getpixel((1, 1)) == (9, 8, 7, 6)


class TestChannels:
    def test_rgb_has_three_channels(self, image: Image) -> None:
        assert image.channels == 3

    def test_grayscale_has_one_channel(self, image: Image) -> None:
        assert image.grayscale().channels == 1

    def test_channels_matches_mode(self, image: Image) -> None:
        expected = {"1": 1, "L": 1, "LA": 2, "P": 1, "RGB": 3, "RGBA": 4, "CMYK": 4}
        assert image.channels == expected[image.mode]


class TestDepth:
    def test_depth_is_per_channel(self, image: Image) -> None:
        """On a Q16 build, an 8-bit PNG reports depth 8 (not 24)."""
        assert image.depth in {8, 16}
        assert image.depth <= 16

    def test_depth_is_positive(self, image: Image) -> None:
        assert image.depth > 0


class TestOtherProperties:
    def test_colorspace(self, image: Image) -> None:
        assert image.colorspace == "sRGB"

    def test_colorspace_changes(self, image: Image) -> None:
        assert image.magick.with_colorspace("Gray").colorspace in {"Gray", "LinearGray"}

    def test_compression_for_png(self, image: Image) -> None:
        assert image.compression in {"Zip", "ZipS", "Undefined", "RLE"}

    def test_compression_quality_is_in_range(self, image: Image) -> None:
        assert 0 <= image.compression_quality <= 100

    def test_filename_is_none_for_bytes(self, image: Image) -> None:
        assert image.filename is None

    def test_filename_is_set_for_paths(self, fixtures: Fixtures) -> None:
        assert fixtures.image("PNG").filename == str(fixtures.path("PNG"))


class TestMetadataDict:
    def test_contains_every_documented_field(self, image: Image) -> None:
        meta = image.metadata()
        expected = {
            "width",
            "height",
            "format",
            "mode",
            "channels",
            "depth",
            "image_type",
            "colorspace",
            "compression",
            "compression_quality",
            "filename",
        }
        assert expected <= set(meta)

    def test_values_agree_with_attributes(self, image: Image) -> None:
        meta = image.metadata()
        assert meta["width"] == image.width
        assert meta["height"] == image.height
        assert meta["format"] == image.format
        assert meta["mode"] == image.mode
        assert meta["channels"] == image.channels
        assert meta["depth"] == image.depth

    def test_low_level_info_agrees(self, image: Image) -> None:
        info = image.magick.info()
        assert info["width"] == image.width
        assert info["colorspace"] == image.colorspace
        assert info["image_type"] == image.image_type


class TestImmutability:
    """Metadata reads must not disturb the image."""

    def test_repeated_reads_are_stable(self, image: Image) -> None:
        first = image.metadata()
        second = image.metadata()
        assert first == second

    def test_metadata_survives_operations(self, image: Image) -> None:
        before = image.metadata()
        image.resize(8, 8).crop(0, 0, 4, 4).grayscale().blur(1, 0.5).flip()
        assert image.metadata() == before


class TestRepr:
    def test_repr_is_informative(self, image: Image) -> None:
        text = repr(image)
        assert "magik.Image" in text
        assert "64x48" in text
        assert "PNG" in text

    def test_version_module(self) -> None:
        info = magik.version()
        assert set(info) == {"version", "quantum_depth", "quantum_range"}
        assert "ImageMagick" in info["version"]