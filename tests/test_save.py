"""Saving to paths and to byte buffers, and reopening what was written."""

from __future__ import annotations

import io
from pathlib import Path

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, WIDTH, Fixtures


class TestSaveToPath:
    def test_png(self, image: Image, tmp_path: Path, need_format, fixtures) -> None:
        need_format("PNG", fixtures)
        out = tmp_path / "out.png"
        image.save(str(out))
        assert out.exists()
        assert Image.open(out).format == "PNG"

    def test_jpeg(self, image: Image, tmp_path: Path, need_format, fixtures) -> None:
        need_format("JPEG", fixtures)
        out = tmp_path / "out.jpg"
        image.save(str(out))
        assert Image.open(out).format == "JPEG"

    def test_explicit_format_overrides_extension(self, image: Image, tmp_path: Path, need_format, fixtures) -> None:
        """An explicit format wins over a misleading filename extension.

        ImageMagick's own writer prefers the extension, so magik applies the
        format through a coder prefix instead. The check is on the bytes rather
        than on the reopened format, because ImageMagick's *reader* also treats
        the extension as a hint and would report the misleading name.
        """
        need_format("PNG", fixtures)
        out = tmp_path / "actually-png.jpg"
        image.save(str(out), format="PNG")
        assert out.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")

    def test_extension_selects_the_encoder(self, image: Image, tmp_path: Path, need_format, fixtures) -> None:
        """Writing a PNG source to .webp must produce real WebP bytes."""
        need_format("WEBP", fixtures)
        out = tmp_path / "out.webp"
        image.save(str(out))
        assert out.read_bytes().startswith(b"RIFF")
        assert Image.open(out.read_bytes()).format == "WEBP"

    def test_accepts_path_object(self, image: Image, tmp_path: Path) -> None:
        out = tmp_path / "out.png"
        image.save(out)
        assert out.exists()

    def test_overwrites_existing_file(self, image: Image, tmp_path: Path) -> None:
        out = tmp_path / "out.png"
        out.write_bytes(b"stale content that must be replaced")
        image.save(str(out))
        assert Image.open(out).size == (WIDTH, HEIGHT)

    def test_save_does_not_change_the_image(self, image: Image, tmp_path: Path) -> None:
        before = image.metadata()
        image.save(str(tmp_path / "out.png"))
        assert image.metadata() == before


class TestSaveToBuffer:
    def test_returns_bytes(self, image: Image) -> None:
        data = image.to_bytes("PNG")
        assert isinstance(data, bytes)
        assert data.startswith(b"\x89PNG\r\n\x1a\n")

    def test_format_argument_selects_encoder(self, image: Image, need_format, fixtures) -> None:
        need_format("JPEG", fixtures)
        assert Image.open(image.to_bytes("JPEG")).format == "JPEG"

    def test_defaults_to_current_format(self, image: Image) -> None:
        assert Image.open(image.to_bytes()).format == "PNG"

    def test_save_to_bytes_alias(self, image: Image) -> None:
        assert image.save_to_bytes("PNG") == image.to_bytes("PNG")

    def test_reopens_at_the_same_size(self, image: Image) -> None:
        assert Image.open(image.to_bytes("PNG")).size == (WIDTH, HEIGHT)


class TestSaveToStream:
    def test_writable_file_like(self, image: Image) -> None:
        buffer = io.BytesIO()
        image.save(buffer, "PNG")
        assert Image.open(buffer.getvalue()).format == "PNG"

    def test_stream_from_opened_file(self, image: Image, tmp_path: Path) -> None:
        out = tmp_path / "streamed.png"
        with out.open("wb") as handle:
            image.save(handle, "PNG")
        assert Image.open(out).size == (WIDTH, HEIGHT)

    def test_no_temporary_file_is_used(self, image: Image, tmp_path: Path) -> None:
        """Saving to a stream must not touch the filesystem."""
        buffer = io.BytesIO()
        image.save(buffer, "PNG")
        assert list(tmp_path.iterdir()) == []


class TestRoundTrip:
    """open -> operate -> save -> open"""

    @pytest.mark.parametrize("fmt", ["PNG", "JPEG", "GIF", "BMP", "TIFF"])
    def test_survives_every_format(self, fixtures: Fixtures, need_format, fmt: str) -> None:
        need_format(fmt, fixtures)
        data = fixtures.data(fmt)
        reopened = Image.open(data)
        again = reopened.resize(reopened.width // 2, reopened.height // 2)
        encoded = again.to_bytes(fmt)
        final = Image.open(encoded)
        assert final.size == (reopened.width // 2, reopened.height // 2)
        assert final.format == fmt

    def test_lossless_png_preserves_pixels(self, image: Image) -> None:
        """PNG is lossless, so a byte-for-byte pixel round trip must be exact."""
        original = image.grayscale().to_bytes("PNG")
        assert Image.open(original).grayscale().to_bytes("PNG") == original

    def test_operation_chain_then_reopen(self, image: Image) -> None:
        result = (
            image.resize(80, 60)
            .crop(5, 5, 75, 55)
            .grayscale()
            .blur(1, 0.5)
            .rotate(90)
        )
        assert result.size == (50, 70)

        reopened = Image.open(result.to_bytes("PNG"))
        assert reopened.size == (50, 70)
        assert reopened.mode == "L"

    def test_multiple_saves_produce_equal_output(self, image: Image) -> None:
        """Saving is repeatable, i.e. it has no hidden side effects."""
        first = image.to_bytes("PNG")
        image.save_to_bytes("JPEG")
        image.blur(2, 1)
        assert image.to_bytes("PNG") == first

    def test_round_trip_through_disk(self, fixtures: Fixtures, tmp_path: Path) -> None:
        source = fixtures.image("PNG")
        out = tmp_path / "chain.png"
        source.resize(120, 90).grayscale().save(str(out))
        final = Image.open(out)
        assert final.size == (120, 90)
        assert final.mode == "L"
        assert final.filename == str(out)