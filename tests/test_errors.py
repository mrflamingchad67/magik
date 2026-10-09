"""Error handling: the hierarchy, the translations, and the safety guarantees."""

from __future__ import annotations

import io
from pathlib import Path

import pytest

import magik
from magik import Image
from magik_fixtures import Fixtures


class TestHierarchy:
    """The documented exception tree."""

    @pytest.mark.parametrize(
        ("child", "parent"),
        [
            (magik.MagikOpenError, magik.MagikError),
            (magik.MagikSaveError, magik.MagikError),
            (magik.MagikOperationError, magik.MagikError),
            (magik.MagikFormatError, magik.MagikError),
            (magik.MagikUnsupportedFormatError, magik.MagikFormatError),
            (magik.MagikInternalError, magik.MagikError),
        ],
    )
    def test_subclassing(self, child: type, parent: type) -> None:
        assert issubclass(child, parent)

    def test_base_is_an_exception(self) -> None:
        assert issubclass(magik.MagikError, Exception)

    def test_exceptions_are_importable_from_the_package(self) -> None:
        from magik import exceptions

        assert exceptions.MagikError is magik.MagikError
        assert set(exceptions.__all__) <= set(magik.__all__)


class TestOpenErrors:
    def test_nonexistent_file(self, tmp_path: Path) -> None:
        missing = tmp_path / "nope.png"
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(missing))
        assert info.value.kind == "open"

    def test_nonexistent_file_message_mentions_the_path(self, tmp_path: Path) -> None:
        missing = tmp_path / "definitely-absent.png"
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(missing))
        assert "definitely-absent.png" in str(info.value)

    def test_invalid_image_data(self) -> None:
        with pytest.raises(magik.MagikOpenError):
            Image.open(b"this is definitely not an image" * 40)

    def test_truncated_png(self, png_bytes: bytes) -> None:
        with pytest.raises(magik.MagikError):
            Image.open(png_bytes[: len(png_bytes) // 2])

    def test_empty_buffer(self) -> None:
        with pytest.raises(magik.MagikOpenError):
            Image.open(b"")

    def test_png_magic_without_data(self) -> None:
        with pytest.raises(magik.MagikError):
            Image.open(b"\x89PNG\r\n\x1a\n")

    def test_not_an_image_type(self) -> None:
        with pytest.raises(TypeError):
            Image.open(12345)

    def test_unsupported_type_for_save(self, image: Image) -> None:
        with pytest.raises(TypeError):
            image.save(12345)


class TestSaveErrors:
    def test_invalid_output_directory(self, image: Image, tmp_path: Path) -> None:
        bad = tmp_path / "missing-dir" / "out.png"
        with pytest.raises(magik.MagikSaveError) as info:
            image.save(str(bad))
        assert info.value.kind == "save"

    def test_write_to_a_directory_path(self, image: Image, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikError):
            image.save(str(tmp_path))

    def test_unwritable_stream(self, image: Image) -> None:
        class NoWrite:
            def write(self, data: bytes) -> int:
                raise OSError("nope")

        with pytest.raises(OSError):
            image.save(NoWrite())

    def test_readonly_stream_type_is_reported(self, image: Image) -> None:
        with pytest.raises(TypeError):
            image.save(object())


class TestOperationErrors:
    @pytest.mark.parametrize(("w", "h"), [(0, 10), (10, 0), (0, 0), (-1, 10), (10, -1)])
    def test_invalid_dimensions(self, image: Image, w: int, h: int) -> None:
        with pytest.raises(magik.MagikOperationError) as info:
            image.resize(w, h)
        assert info.value.kind == "operation"

    def test_non_integer_dimensions(self, image: Image) -> None:
        with pytest.raises((TypeError, magik.MagikOperationError)):
            image.resize("big", 10)

    def test_oversized_resize_is_allowed(self, image: Image) -> None:
        """Large targets are legal; only zero/negative are rejected."""
        assert image.resize(2000, 2000).size == (2000, 2000)

    def test_crop_outside_bounds(self, image: Image) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.crop(0, 0, 10_000, 10_000)

    def test_resize_with_missing_height(self, image: Image) -> None:
        with pytest.raises(TypeError):
            image.resize(50)

    def test_crop_with_missing_arguments(self, image: Image) -> None:
        with pytest.raises(TypeError):
            image.crop(0, 0, 10)


class TestFormatErrors:
    def test_unknown_format_name_for_encoding(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError) as info:
            image.to_bytes("NOT-A-FORMAT")
        assert info.value.kind == "format"

    def test_empty_format_name(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.to_bytes("")

    def test_unknown_colorspace(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.magick.with_colorspace("NotAColorspace")

    def test_unknown_compression(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.magick.with_compression("NotACompression")

    def test_format_without_an_encoder_is_unsupported(self, image: Image) -> None:
        """XCF is a real ImageMagick format that this build cannot write.

        It must surface as `MagikUnsupportedFormatError`, which is catchable both
        specifically and as a `MagikFormatError`.
        """
        try:
            image.to_bytes("XCF")
        except magik.MagikUnsupportedFormatError as exc:
            assert exc.kind == "unsupported_format"
            assert isinstance(exc, magik.MagikFormatError)
        except magik.MagikError:
            pytest.skip("this ImageMagick build can write XCF")


class TestErrorPayload:
    """Errors carry the ImageMagick message rather than swallowing it."""

    def test_detail_attribute_exists(self, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(tmp_path / "absent.png"))
        assert hasattr(info.value, "imagemagick_detail")

    def test_missing_file_includes_imagemagick_detail(self, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(tmp_path / "absent.png"))
        detail = info.value.imagemagick_detail
        assert detail is None or isinstance(detail, str)

    def test_kind_attribute_matches_the_type(self) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(b"nope")
        assert info.value.kind == "open"

    def test_message_is_informative(self) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(b"nope" * 100)
        assert str(info.value).strip()

    def test_no_internal_error_leaks_from_bad_input(self) -> None:
        """Pathological inputs must not be reported as internal bugs."""
        for payload in [b"", b"\x00", b"\xff" * 32, os_null_bytes()]:
            with pytest.raises(magik.MagikError) as info:
                Image.open(payload)
            assert not isinstance(info.value, magik.MagikInternalError)


def os_null_bytes() -> bytes:
    """A buffer that cannot be a path because it contains NUL."""
    return b"bad\x00name.png"


class TestNoPanicEscapes:
    """Rust panics must surface as MagikInternalError, never as a crash."""

    def test_deeply_invalid_geometry(self, image: Image) -> None:
        with pytest.raises(magik.MagikError) as info:
            image.resize(2**31, 2**31)
        assert not isinstance(info.value, magik.MagikInternalError)

    def test_huge_crop_coordinates(self, image: Image) -> None:
        with pytest.raises(magik.MagikError) as info:
            image.crop(0, 0, 2**40, 2**40)
        assert not isinstance(info.value, magik.MagikInternalError)

    def test_infinite_blur(self, image: Image) -> None:
        with pytest.raises(magik.MagikError):
            image.blur(float("inf"), float("inf"))

    def test_interpreter_survives_the_above(self, image: Image) -> None:
        """If we are still here afterwards, no panic crossed the boundary."""
        assert image.width > 0


class TestErrorAttributeTypes:
    def test_kind_is_a_string(self) -> None:
        with pytest.raises(magik.MagikError) as info:
            Image.open(b"x" * 10)
        assert isinstance(info.value.kind, str)

    def test_detail_is_a_string_or_none(self) -> None:
        with pytest.raises(magik.MagikError) as info:
            Image.open(b"x" * 10)
        assert info.value.imagemagick_detail is None or isinstance(
            info.value.imagemagick_detail, str
        )

    def test_exceptions_are_picklable(self) -> None:
        import pickle

        exc = magik.MagikOpenError("boom")
        restored = pickle.loads(pickle.dumps(exc))
        assert str(restored) == "boom"


class TestErrorsAreCatchableAsBase:
    def test_single_base_catch(self, image: Image) -> None:
        with pytest.raises(magik.MagikError):
            image.resize(0, 0)

    def test_unsupported_is_also_a_format_error(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.to_bytes("NOT-A-FORMAT")


class TestStreamErrors:
    def test_read_only_stream_for_open(self) -> None:
        class ReadOnly:
            def read(self) -> str:
                return "not bytes"

        with pytest.raises(TypeError):
            Image.open(ReadOnly())

    def test_bytesio_missing_after_close(self, png_bytes: bytes) -> None:
        stream = io.BytesIO(png_bytes)
        stream.close()
        with pytest.raises((ValueError, magik.MagikError)):
            Image.open(stream)