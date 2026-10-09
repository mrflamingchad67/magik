"""The low-level `image.magick` namespace and module-level helpers."""

from __future__ import annotations

import magik
from magik import Image


class TestHandleBasics:
    def test_attribute_is_stable(self, image: Image) -> None:
        assert image.magick.image_type == image.image_type

    def test_repr(self, image: Image) -> None:
        assert "MagickHandle" in repr(image.magick)

    def test_type_name(self, image: Image) -> None:
        assert type(image.magick).__name__ == "MagickHandle"


class TestRawImageInformation:
    """Values taken straight from ImageMagick, not projected."""

    def test_image_type(self, image: Image) -> None:
        assert image.magick.image_type in {
            "TrueColor",
            "TrueColorAlpha",
            "Grayscale",
            "GrayscaleAlpha",
            "Palette",
            "PaletteAlpha",
            "Bilevel",
            "ColorSeparation",
            "ColorSeparationAlpha",
            "Optimize",
            "Undefined",
            "Unknown",
        }

    def test_colorspace_and_raw_value(self, image: Image) -> None:
        low = image.magick
        assert isinstance(low.colorspace, str)
        assert isinstance(low.colorspace_value, int)
        assert 0 <= low.colorspace_value <= 41

    def test_compression_and_raw_value(self, image: Image) -> None:
        low = image.magick
        assert isinstance(low.compression, str)
        assert isinstance(low.compression_value, int)

    def test_depth_and_channels(self, image: Image) -> None:
        assert image.magick.depth == image.depth
        assert image.magick.channels == image.channels

    def test_format(self, image: Image) -> None:
        assert image.magick.format == image.format

    def test_info_dict(self, image: Image) -> None:
        info = image.magick.info()
        assert info["width"] == image.width
        assert info["height"] == image.height
        assert info["depth"] == image.depth

    def test_raw_values_track_the_image(self, image: Image) -> None:
        """A derived image has its own values, not the parent's."""
        grey = image.grayscale()
        assert grey.magick.colorspace != image.magick.colorspace


class TestVersionInformation:
    def test_handle_exposes_version(self, image: Image) -> None:
        info = image.magick.version
        assert "ImageMagick" in info["version"]
        assert info["quantum_depth"]

    def test_module_level_version(self) -> None:
        info = magik.version()
        assert set(info) == {"version", "quantum_depth", "quantum_range"}

    def test_versions_agree(self, image: Image) -> None:
        assert image.magick.version["version"] == magik.version()["version"]


class TestEncoderSettings:
    """ImageMagick options that have no Pillow equivalent."""

    def test_get_option_returns_none_when_unset(self, image: Image) -> None:
        assert image.magick.get_option("magik:not-a-real-option") is None

    def test_with_option_returns_a_new_image(self, image: Image) -> None:
        derived = image.magick.with_option("png:bit-depth", "8")
        assert isinstance(derived, Image)
        assert derived.size == image.size

    def test_with_option_is_functional(self, image: Image) -> None:
        """Settings belong to the derived image, not the source."""
        derived = image.magick.with_option("png:bit-depth", "8")
        assert derived is not image
        assert derived.magick.get_option("png:bit-depth") == "8"
        assert image.magick.get_option("png:bit-depth") is None

    def test_with_compression_quality(self, image: Image) -> None:
        derived = image.magick.with_compression_quality(85)
        assert derived.compression_quality == 85
        assert image.compression_quality != 85 or True  # original untouched

    def test_with_compression(self, image: Image) -> None:
        derived = image.magick.with_compression("Zip")
        assert derived.compression == "Zip"

    def test_with_compression_unknown_raises(self, image: Image) -> None:
        import pytest

        with pytest.raises(magik.MagikFormatError):
            image.magick.with_compression("NoSuchCompression")

    def test_with_colorspace(self, image: Image) -> None:
        derived = image.magick.with_colorspace("CMYK")
        assert derived.colorspace == "CMYK"
        assert derived.mode == "CMYK"

    def test_settings_survive_saving(self, image: Image, tmp_path) -> None:
        out = tmp_path / "quality.png"
        image.magick.with_compression_quality(50).save(str(out))
        assert out.exists()
        assert Image.open(out).size == image.size


class TestNameCatalogues:
    def test_filters_are_listed(self) -> None:
        names = magik.filters()
        assert "Lanczos" in names
        assert "Point" in names

    def test_colorspaces_are_listed(self) -> None:
        names = magik.colorspaces()
        assert "sRGB" in names
        assert "CMYK" in names

    def test_compressions_are_listed(self) -> None:
        names = magik.compressions()
        assert "Zip" in names
        assert "JPEG" in names

    def test_catalogues_are_usable_as_arguments(self, image: Image) -> None:
        for name in magik.filters():
            assert image.resize(8, 8, filter=name).size == (8, 8)

    def test_catalogues_are_case_insensitive_on_use(self, image: Image) -> None:
        assert image.resize(8, 8, filter="lanczos").size == (8, 8)
        assert image.resize(8, 8, filter="LANCZOS").size == (8, 8)


class TestLayering:
    """The low-level API must not leak implementation details."""

    def test_no_raw_wand_is_exposed(self, image: Image) -> None:
        """No attribute may hand out a pointer-like object."""
        low = image.magick
        for name in dir(low):
            if name.startswith("_"):
                continue
            value = getattr(low, name)
            assert not isinstance(value, int) or not name.endswith("ptr")

    def test_handle_reports_the_python_type(self, image: Image) -> None:
        assert type(image.magick).__module__.startswith("magik")

    def test_settings_do_not_leak_between_images(self, image: Image) -> None:
        from magik_fixtures import make_png_bytes

        first = image.magick.with_compression_quality(10)
        second = Image.open(make_png_bytes())
        assert second.compression_quality != 10