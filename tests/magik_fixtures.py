"""Deterministic image fixtures for the magik test-suite.

Design goals
------------

* **No binary assets in the repository.** Every fixture is generated at run time.
* **Deterministic.** The same bytes every run, so a failure is reproducible.
* **Not circular where it matters.** PNG and BMP are assembled *by hand* from the
  standard library, so decoding them genuinely exercises magik's read path
  against data it did not produce. JPEG, GIF, TIFF and WebP fixtures are produced
  by ImageMagick itself (encoding those by hand is not practical), which is
  recorded here explicitly.
* **Capability-aware.** Which formats exist depends on the delegates the
  installed ImageMagick was built with, so helpers detect support and tests skip
  rather than fail when a delegate is genuinely missing.
"""

from __future__ import annotations

import struct
import zlib
from dataclasses import dataclass
from pathlib import Path

import magik

#: Size of every generated fixture, in pixels.
WIDTH = 64
HEIGHT = 48

#: Formats the suite exercises. PNG and BMP are hand-built; the rest are
#: produced by ImageMagick from the hand-built PNG.
FORMATS = ("PNG", "JPEG", "GIF", "BMP", "TIFF", "WEBP")


# ---------------------------------------------------------------------------
# Hand-assembled images (independent of ImageMagick)
# ---------------------------------------------------------------------------


def _pixel(x: int, y: int) -> bytes:
    """A deterministic RGB triple with enough gradient for resampling."""
    return bytes(((x * 4) % 256, (y * 5) % 256, ((x + y) * 3) % 256))


def make_png_bytes(width: int = WIDTH, height: int = HEIGHT) -> bytes:
    """Build a valid 8-bit truecolour PNG using only `zlib`."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter type 0 (None) for every scanline
        for x in range(width):
            raw += _pixel(x, y)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def make_bmp_bytes(width: int = WIDTH, height: int = HEIGHT) -> bytes:
    """Build a valid 24-bit uncompressed BMP using only `struct`."""
    row_stride = ((width * 3 + 3) // 4) * 4  # rows are padded to 4 bytes
    pixels = bytearray()
    for y in range(height - 1, -1, -1):  # BMP scanlines are bottom-up
        row = bytearray()
        for x in range(width):
            r, g, b = _pixel(x, y)
            row += bytes((b, g, r))  # BMP stores BGR
        row += b"\x00" * (row_stride - len(row))
        pixels += row

    file_header = b"BM" + struct.pack("<IHHI", 14 + 40 + len(pixels), 0, 0, 54)
    info_header = struct.pack(
        "<IiiHHIIiiII",
        40,  # header size
        width,
        height,
        1,  # planes
        24,  # bits per pixel
        0,  # BI_RGB (uncompressed)
        len(pixels),
        2835,  # ~72 DPI
        2835,
        0,
        0,
    )
    return file_header + info_header + bytes(pixels)


#: A canonical 1x1 GIF89a. Hand-written, so it independently proves that magik
#: decodes externally produced GIF data rather than only its own output.
MINIMAL_GIF = (
    b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff"
    b"!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00"
    b"\x00\x02\x02D\x01\x00;"
)


# ---------------------------------------------------------------------------
# Fixture set
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Fixtures:
    """A generated fixture set on disk, plus access to the bytes behind it."""

    directory: Path

    def path(self, fmt: str) -> Path:
        """Filesystem path of the fixture for `fmt`."""
        suffix = {"JPEG": "jpg", "TIFF": "tif"}.get(fmt.upper(), fmt.lower())
        return self.directory / f"sample.{suffix}"

    def data(self, fmt: str) -> bytes:
        """Raw encoded bytes of the fixture for `fmt`."""
        return self.path(fmt).read_bytes()

    def image(self, fmt: str = "PNG") -> "magik.Image":
        """Open the fixture for `fmt`."""
        return magik.Image.open(self.path(fmt))

    def existing(self) -> set[str]:
        """Formats whose fixture file was actually produced."""
        return {fmt for fmt in FORMATS if self.path(fmt).exists()}


def build(directory: Path) -> Fixtures:
    """Generate every fixture into `directory`.

    Formats this ImageMagick build cannot write are simply left absent; the
    tests that want them consult :func:`skip_reason`.
    """
    directory.mkdir(parents=True, exist_ok=True)
    fixtures = Fixtures(directory=directory)

    # 1. Hand-assembled sources.
    (directory / "sample.png").write_bytes(make_png_bytes())
    (directory / "sample.bmp").write_bytes(make_bmp_bytes())
    (directory / "tiny.gif").write_bytes(MINIMAL_GIF)

    # 2. Everything else is transcoded from the hand-built PNG.
    source = magik.Image.open(directory / "sample.png")
    for fmt in ("JPEG", "GIF", "TIFF", "WEBP"):
        try:
            source.save(str(fixtures.path(fmt)))
        except magik.MagikError:
            continue  # no delegate for this format in this build
    return fixtures


# ---------------------------------------------------------------------------
# Capability detection
# ---------------------------------------------------------------------------


def available(fixtures: "Fixtures | None" = None) -> set[str]:
    """The formats this ImageMagick build can both write and read."""
    usable = set()
    for fmt in FORMATS:
        if fixtures is not None and not fixtures.path(fmt).exists():
            continue
        try:
            magik.Image.open(make_png_bytes()).to_bytes(fmt)
        except magik.MagikError:
            continue
        usable.add(fmt)
    return usable


def skip_reason(fmt: str, fixtures: "Fixtures | None" = None) -> str | None:
    """Why `fmt` cannot be tested, or `None` when it can.

    Delegate availability depends on how ImageMagick was built, so tests skip
    rather than fail when a format is genuinely unavailable.
    """
    fmt = fmt.upper()
    if fixtures is not None and not fixtures.path(fmt).exists():
        return f"{fmt} is not supported by the installed ImageMagick build"
    try:
        magik.Image.open(make_png_bytes()).to_bytes(fmt)
    except magik.MagikError as exc:
        return f"{fmt} is not supported by the installed ImageMagick build ({exc})"
    return None