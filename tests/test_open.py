"""Loading images from paths and byte buffers."""

from __future__ import annotations

import io
import os
from pathlib import Path

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, MINIMAL_GIF, WIDTH, Fixtures


class TestOpenFromPath:
    """`Image.open(path)`"""

    def test_opens_each_format(self, fixtures: Fixtures, need_format) -> None:
        for fmt in ("PNG", "JPEG", "GIF", "BMP", "TIFF"):
            need_format(fmt, fixtures)
            image = Image.open(fixtures.path(fmt))
            assert image.size == (WIDTH, HEIGHT), f"{fmt} had the wrong size"

    def test_png_is_hand_assembled_and_decodes(self, fixtures: Fixtures) -> None:
        """PNG fixtures are built by the test suite, not by ImageMagick."""
        raw = fixtures.path("PNG").read_bytes()
        assert raw.startswith(b"\x89PNG\r\n\x1a\n")
        assert Image.open(raw).size == (WIDTH, HEIGHT)

    def test_bmp_is_hand_assembled_and_decodes(self, fixtures: Fixtures) -> None:
        raw = fixtures.path("BMP").read_bytes()
        assert raw.startswith(b"BM")
        assert Image.open(raw).size == (WIDTH, HEIGHT)

    def test_hand_written_gif_decodes(self, tiny_gif: Image) -> None:
        assert tiny_gif.format == "GIF"
        assert tiny_gif.size == (1, 1)

    def test_reports_filename(self, fixtures: Fixtures) -> None:
        assert Image.open(fixtures.path("PNG")).filename == str(fixtures.path("PNG"))

    def test_accepts_str_and_pathlike(self, fixtures: Fixtures) -> None:
        path = fixtures.path("PNG")
        assert Image.open(str(path)).size == (WIDTH, HEIGHT)
        assert Image.open(Path(path)).size == (WIDTH, HEIGHT)

    def test_module_level_open_matches(self, fixtures: Fixtures) -> None:
        path = fixtures.path("PNG")
        assert magik.open(path).size == Image.open(path).size

    def test_relative_path(self, fixtures: Fixtures, monkeypatch) -> None:
        monkeypatch.chdir(fixtures.directory)
        assert Image.open("sample.png").size == (WIDTH, HEIGHT)


class TestOpenFromBytes:
    """`Image.open(bytes)`"""

    def test_png_bytes(self, png_bytes: bytes) -> None:
        assert Image.open(png_bytes).format == "PNG"

    @pytest.mark.parametrize("wrap", [bytes, bytearray, memoryview])
    def test_accepts_any_buffer(self, png_bytes: bytes, wrap) -> None:
        assert Image.open(wrap(png_bytes)).size == (WIDTH, HEIGHT)

    def test_from_bytes_staticmethod(self, png_bytes: bytes) -> None:
        assert Image.from_bytes(png_bytes).size == (WIDTH, HEIGHT)

    def test_accepts_file_like(self, png_bytes: bytes) -> None:
        assert Image.open(io.BytesIO(png_bytes)).size == (WIDTH, HEIGHT)

    def test_file_like_is_read_once(self, png_bytes: bytes) -> None:
        """A stream is consumed, not re-read."""
        stream = io.BytesIO(png_bytes)
        assert Image.open(stream).size == (WIDTH, HEIGHT)
        assert stream.read() == b""

    def test_gif_bytes(self) -> None:
        assert Image.open(MINIMAL_GIF).format == "GIF"


class TestOpenEdgeCases:
    """Unusual but valid ways to call `open`."""

    def test_empty_bytes_fails_cleanly(self) -> None:
        with pytest.raises(magik.MagikOpenError):
            Image.open(b"")

    def test_large_image_is_lazy_about_pixels(self) -> None:
        """Opening does not require a full pixel copy into Python."""
        from magik_fixtures import make_png_bytes

        # 512x512 of generated gradient: opening must not allocate a Python list.
        big = make_png_bytes(512, 512)
        assert Image.open(big).size == (512, 512)

    def test_directory_path_is_rejected(self, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikError):
            Image.open(str(tmp_path))

    def test_no_temp_files_are_created(self, fixtures: Fixtures, tmp_path: Path) -> None:
        """In-memory work must not spill into the filesystem."""
        before = set(os.listdir(tmp_path))
        data = Image.open(fixtures.path("PNG")).resize(16, 16).to_bytes("PNG")
        assert data
        assert set(os.listdir(tmp_path)) == before