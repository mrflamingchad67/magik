"""Stage 03: image I/O, format selection, and the behaviour of ImageMagick's writer.

The tests here deliberately check **logical pixel correctness**, not the storage
representation ImageMagick happens to pick. See `TestPngPaletteReclassification`
for why that distinction matters.
"""

from __future__ import annotations

import io
from pathlib import Path

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, WIDTH, Fixtures, make_png_bytes

#: Formats the Stage 03 specification requires to be exercised.
STAGE_03_FORMATS = ("PNG", "JPEG", "WEBP", "GIF", "TIFF", "BMP")

#: Formats verified to reproduce every sample exactly.
LOSSLESS = ("PNG", "TIFF", "BMP")

#: Formats that quantise by design: they round trip a photo, but not a gradient
#: sample-for-sample. GIF reduces to a 256-entry palette and WebP defaults to
#: lossy compression, so neither is asserted to be exact.
QUANTISED = ("GIF", "WEBP")

#: BMP is reported by ImageMagick with its bit-depth variant. A hand-built
#: 24-bit file therefore reads back as `BMP3`, not `BMP`.
BMP_VARIANTS = ("BMP", "BMP2", "BMP3")


def nul_bytes() -> bytes:
    """A buffer that cannot be a valid path on any platform."""
    return b"bad\x00name.png"


def channels_of(image: Image) -> int:
    """Samples per pixel, inferred from the flat pixel buffer."""
    return len(image.pixels()) // (image.width * image.height)


def rgb_of(image: Image) -> bytes:
    """An image's pixels reduced to RGB triplets.

    A fully opaque image may legitimately come back with one sample per channel
    fewer than it went in with: ImageMagick discards an alpha channel that
    carries no information, and reducing both sides the same way compares what
    the images actually *show* rather than how many bytes they happen to use.
    Transparency is never dropped here — only a provably redundant alpha.
    """
    data = image.pixels()
    if channels_of(image) == 4 and all(data[i] == 255 for i in range(3, len(data), 4)):
        return bytes(value for index, value in enumerate(data) if index % 4 != 3)
    return data


def max_channel_delta(left: Image, right: Image) -> int:
    """Largest per-channel difference between two same-sized images."""
    a = left.pixels()
    b = right.pixels()
    assert len(a) == len(b), "pixel counts differ"
    return max((abs(int(x) - int(y)) for x, y in zip(a, b)), default=0)


def detailed_image(mode: str, width: int = WIDTH, height: int = HEIGHT) -> Image:
    """A deterministic image with enough colour variety to resist palette-ising.

    Uniform and smoothly-smoothed images are exactly the ones ImageMagick
    rewrites as palettes, which is useful for testing that path but useless for
    asserting that a container preserved the storage type. The arithmetic ramp
    below produces many distinct colours and non-uniform alpha, so a codec that
    must keep truecolour has no cheaper option.
    """
    channels = {"RGB": 3, "RGBA": 4}[mode]
    data = bytearray()
    for y in range(height):
        for x in range(width):
            data += bytes((x * 37 + y * 11 + c * 53) % 256 for c in range(channels))
    fill = tuple([0] * channels)
    return Image.new(mode, (width, height), fill).putpixels((0, 0), (width, height), bytes(data))


# ---------------------------------------------------------------------------
# §4 -- format selection on save
# ---------------------------------------------------------------------------


class TestFormatSelection:
    """The format argument, the filename extension, and who wins."""

    @pytest.mark.parametrize("fmt", STAGE_03_FORMATS)
    def test_extension_selects_the_encoder(self, image: Image, fmt: str, tmp_path: Path, need_format) -> None:
        need_format(fmt)
        suffix = {"JPEG": "jpg", "TIFF": "tif"}.get(fmt, fmt.lower())
        out = tmp_path / f"by-extension.{suffix}"
        image.save(str(out))
        assert Image.open(out).format == fmt

    @pytest.mark.parametrize("fmt", ["PNG", "JPEG", "WEBP"])
    def test_explicit_format_overrides_extension(self, image: Image, fmt: str, tmp_path: Path, need_format) -> None:
        """`save("x.jpg", format="png")` must write PNG, not JPEG.

        Checked on the file's magic bytes rather than on the reopened format:
        ImageMagick's *reader* also treats the extension as a hint, so
        reopening is not a trustworthy witness here.
        """
        need_format(fmt)
        out = tmp_path / f"mislabelled-{fmt.lower()}.jpg"
        image.save(str(out), format=fmt)
        data = out.read_bytes()
        assert data == image.to_bytes(fmt), f"{fmt} bytes differ from an explicit encode"

    def test_extension_and_format_agree(self, image: Image, tmp_path: Path) -> None:
        out = tmp_path / "agree.png"
        image.save(str(out), format="png")
        assert out.read_bytes() == image.to_bytes("png")

    def test_format_is_case_insensitive(self, image: Image, tmp_path: Path) -> None:
        lower = tmp_path / "lower.png"
        upper = tmp_path / "upper.png"
        image.save(str(lower), format="png")
        image.save(str(upper), format="PNG")
        assert lower.read_bytes() == upper.read_bytes()

    def test_unknown_extension_keeps_the_source_format(self, image: Image, tmp_path: Path) -> None:
        """A `.weirdext` file has no coder, so the decoded format is kept."""
        out = tmp_path / "image.weirdext"
        image.save(str(out))
        assert out.read_bytes() == image.to_bytes("PNG")

    def test_extensionless_path_keeps_the_source_format(self, image: Image, tmp_path: Path) -> None:
        out = tmp_path / "noextension"
        image.save(str(out))
        assert out.read_bytes() == image.to_bytes("PNG")

    def test_saving_does_not_mutate_the_image_format(self, image: Image, tmp_path: Path) -> None:
        """Stage 01 regression: encode must restore the image's identity."""
        before = image.format
        for name in ("one.jpg", "two.png", "three.webp", "four.gif"):
            try:
                image.save(str(tmp_path / name))
            except magik.MagikError:
                continue
        assert image.format == before


class TestUnknownFormatIsRejected:
    """A typo must be reported, not silently ignored."""

    @pytest.mark.parametrize("bad", ["NOT-A-FORMAT", "BOGUSFMT", "J", "x.y", "12", "png2"])
    def test_save_to_path_rejects_it(self, image: Image, bad: str, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikFormatError) as info:
            image.save(str(tmp_path / f"out-{bad}.png"), format=bad)
        assert bad.upper() in str(info.value).upper()

    @pytest.mark.parametrize("bad", ["NOT-A-FORMAT", "BOGUSFMT", "J", "12"])
    def test_to_bytes_rejects_it(self, image: Image, bad: str) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.to_bytes(bad)

    @pytest.mark.parametrize("bad", ["NOT-A-FORMAT", "BOGUSFMT", "J"])
    def test_unknown_format_never_falls_back_to_the_extension(
        self, image: Image, bad: str, tmp_path: Path
    ) -> None:
        """Regression: ImageMagick's writer ignores unrecognised coder prefixes.

        `save("out.png", format="NOT-A-FORMAT")` used to write a valid PNG and
        report success, so a caller who mistyped the format got a silently wrong
        file. magik validates the format first, so this must now raise *and*
        leave nothing behind.
        """
        out = tmp_path / f"fallback-{bad}.png"
        with pytest.raises(magik.MagikFormatError):
            image.save(str(out), format=bad)
        assert not out.exists(), "a rejected format must not write a file"

    def test_the_message_explains_rather_than_naming_a_c_function(self, image: Image) -> None:
        """`MagickSetImageFormat` records no reason, so magik supplies one.

        Reporting "MagickSetImageFormat failed" reads like a diagnosis while
        telling the caller nothing at all.
        """
        with pytest.raises(magik.MagikFormatError) as info:
            image.to_bytes("NOT-A-FORMAT")
        message = str(info.value)
        assert "MagickSetImageFormat" not in message
        assert "NOT-A-FORMAT" in message
        assert len(message) > len("unknown or unusable image format")

    @pytest.mark.parametrize("blank", ["", "   ", "\t\n"])
    def test_blank_format_names_are_rejected_everywhere(self, image: Image, blank: str, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.to_bytes(blank)
        with pytest.raises(magik.MagikFormatError):
            image.save(str(tmp_path / "blank.png"), format=blank)


class TestCodersThatWriteNothing:
    """Some ImageMagick coders report success and produce no file."""

    def test_null_coder_reports_a_save_error(self, image: Image, tmp_path: Path) -> None:
        """Regression: this used to surface as a bare `FileNotFoundError`.

        The `NULL:` coder legitimately writes nothing, and ImageMagick still
        returns success. The user must get a magik error that says so, not an
        unrelated filesystem exception from whatever they did next.
        """
        out = tmp_path / "swallowed.png"
        try:
            image.save(str(out), format="NULL")
        except magik.MagikSaveError as exc:
            assert exc.kind == "save"
            assert not out.exists()
        except magik.MagikFormatError:
            pytest.skip("this ImageMagick build does not know the NULL coder")
        else:
            pytest.fail("the NULL coder should not report a successful file write")


# ---------------------------------------------------------------------------
# §10 -- round trips
# ---------------------------------------------------------------------------


class TestRoundTrip:
    """Bytes in, bytes out, pixels preserved."""

    @pytest.mark.parametrize("fmt", LOSSLESS)
    def test_lossless_formats_preserve_pixels(self, image: Image, fmt: str, need_format) -> None:
        need_format(fmt)
        restored = Image.from_bytes(image.to_bytes(fmt))
        assert restored.size == image.size
        assert restored.pixels() == image.pixels(), f"{fmt} altered the pixels"

    @pytest.mark.parametrize("fmt", QUANTISED)
    def test_quantised_formats_stay_visually_close(self, image: Image, fmt: str, need_format) -> None:
        """GIF and WebP quantise by design; they must still look the same.

        Neither is required to reproduce samples exactly (§10 forbids demanding
        that of lossy formats), so each is asserted against a tolerance justified
        by how it works: GIF keeps a 256-entry palette, which is enough for this
        gradient to survive intact, whereas WebP applies lossy DCT compression
        and is only expected to stay recognisable.
        """
        need_format(fmt)
        restored = Image.from_bytes(image.to_bytes(fmt))
        assert restored.size == image.size
        if fmt == "GIF":
            assert rgb_of(restored) == rgb_of(image)
        else:
            assert max_channel_delta(restored, image) < 200

    def test_jpeg_is_lossy_but_close(self, image: Image, need_format) -> None:
        """JPEG must not be required to reproduce exact pixels (§10).

        A broad gradient survives JPEG quantisation well enough that the result
        is still recognisably the same image, which is the property worth
        asserting. There is no quality knob on `to_bytes` yet, so this also
        documents the default ImageMagick picks.
        """
        need_format("JPEG")
        restored = Image.from_bytes(image.to_bytes("JPEG"))
        assert restored.size == image.size
        assert max_channel_delta(restored, image) < 64

    @pytest.mark.parametrize("fmt", STAGE_03_FORMATS)
    def test_file_round_trip_preserves_size(self, image: Image, fmt: str, tmp_path: Path, need_format) -> None:
        need_format(fmt)
        suffix = {"JPEG": "jpg", "TIFF": "tif"}.get(fmt, fmt.lower())
        out = tmp_path / f"round.{suffix}"
        image.save(str(out))
        restored = Image.open(out)
        assert restored.size == image.size
        assert restored.format == fmt

    def test_pixels_to_bytes_to_pixels_is_exact_for_png(self, image: Image) -> None:
        """pixels -> image -> PNG -> image -> pixels, with no drift."""
        first = image.pixels()
        assert Image.from_bytes(image.to_bytes("PNG")).pixels() == first

    def test_temporary_directory_is_not_polluted(self, image: Image, tmp_path: Path) -> None:
        image.save(str(tmp_path / "clean.png"))
        image.to_bytes("JPEG")
        assert [p.name for p in tmp_path.iterdir()] == ["clean.png"]


# ---------------------------------------------------------------------------
# §11 -- PNG palette reclassification
# ---------------------------------------------------------------------------


class TestPngPaletteReclassification:
    """ImageMagick re-encodes repetitive images as palette PNGs.

    This is **ImageMagick's optimisation, not a magik defect**: a flat rectangle
    has so few distinct colours that a palette PNG is far smaller, so that is
    what it writes. What magik must guarantee is that the decoded image still
    *means* the same thing — the storage representation is ImageMagick's call,
    the pixels are ours.
    """

    def test_uniform_opaque_image_stays_visually_identical(self, tmp_path: Path) -> None:
        """A flat opaque RGBA rectangle comes back as a 1-bit palette PNG.

        Every alpha sample was 255, so the alpha channel carried no information
        and ImageMagick is right to discard it. The colour must survive intact.
        """
        flat = Image.new("RGBA", (16, 16), (255, 0, 0, 255))
        out = tmp_path / "flat.png"
        flat.save(str(out))
        restored = Image.open(out)

        assert restored.size == flat.size
        assert restored.getpixel((0, 0)) == (255, 0, 0)
        assert rgb_of(restored) == rgb_of(flat)

    def test_real_transparency_survives_exactly(self, tmp_path: Path) -> None:
        """Regression: transparency used to be dropped on the way back.

        ImageMagick keeps an alpha channel whenever some pixel is genuinely not
        opaque, reporting the image as `PaletteAlpha`. magik used to resolve
        every palette image to 3-sample RGB regardless, which silently turned a
        half-transparent pixel opaque and a fully transparent one black.
        """
        half = Image.new("RGBA", (8, 8), (10, 20, 30, 128))
        restored = Image.open(half.to_bytes("PNG"))
        assert restored.getpixel((0, 0)) == (10, 20, 30, 128)
        assert restored.pixels() == half.pixels()

    def test_fully_transparent_survives_exactly(self) -> None:
        transparent = Image.new("RGBA", (8, 8), (0, 0, 0, 0))
        restored = Image.from_bytes(transparent.to_bytes("PNG"))
        assert restored.getpixel((0, 0)) == (0, 0, 0, 0)
        assert restored.pixels() == transparent.pixels()

    def test_a_photographic_image_is_not_reclassified(self, image: Image) -> None:
        """Only *repetitive* images get the palette treatment.

        A real gradient must come back as truecolour, so this guards against
        magik accidentally forcing a palette path for everything.
        """
        restored = Image.from_bytes(image.to_bytes("PNG"))
        assert restored.mode == "RGB"
        assert restored.pixel_mode == "RGB"
        assert restored.pixels() == image.pixels()

    def test_lossless_native_format_preserves_the_internal_type(self, tmp_path: Path) -> None:
        """MIFF is ImageMagick's own container and keeps the storage type.

        Documented in the README as the option to reach for when exact type
        preservation matters more than interoperability.
        """
        flat = Image.new("RGBA", (16, 16), (255, 0, 0, 255))
        out = tmp_path / "flat.miff"
        flat.save(str(out))
        restored = Image.open(out)
        assert restored.format == "MIFF"
        assert restored.mode == "RGBA"
        assert restored.pixels() == flat.pixels()

    def test_opaque_and_transparent_images_report_differently(self) -> None:
        """`pixel_mode` distinguishes the two cases even though `mode` does not.

        `mode` reports `"P"` for both, matching Pillow's convention of tracking
        transparency outside the mode string. `pixel_mode` is what magik actually
        transfers, so it reflects the real sample count.
        """
        opaque = Image.from_bytes(Image.new("RGBA", (8, 8), (1, 2, 3, 255)).to_bytes("PNG"))
        see_through = Image.from_bytes(Image.new("RGBA", (8, 8), (1, 2, 3, 64)).to_bytes("PNG"))
        assert opaque.mode == see_through.mode == "P"
        assert channels_of(opaque) == 3
        assert channels_of(see_through) == 4


# ---------------------------------------------------------------------------
# §12 -- error handling
# ---------------------------------------------------------------------------


class TestIoErrorHandling:
    @pytest.mark.parametrize(
        "payload",
        [b"", b"\x00", b"not an image" * 50, b"\x89PNG\r\n\x1a\n", nul_bytes()],
    )
    def test_invalid_input_never_yields_an_empty_image(self, payload: bytes) -> None:
        with pytest.raises(magik.MagikOpenError):
            Image.open(payload)

    @pytest.mark.parametrize("payload", [b"", b"\x00" * 64, b"garbage" * 100])
    def test_from_bytes_rejects_invalid_data(self, payload: bytes) -> None:
        with pytest.raises(magik.MagikError):
            Image.from_bytes(payload)

    def test_corrupt_png_reports_image_failure_not_a_crash(self, png_bytes: bytes) -> None:
        corrupt = bytearray(png_bytes)
        corrupt[-40:-20] = b"\xde\xad\xbe\xef" * 5  # smash the compressed data
        with pytest.raises(magik.MagikError) as info:
            Image.open(bytes(corrupt))
        assert not isinstance(info.value, magik.MagikInternalError)

    def test_nonexistent_path_names_the_file(self, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(tmp_path / "absent.png"))
        assert "absent.png" in str(info.value)

    def test_imagemagick_reason_is_preserved_when_it_gives_one(self, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikOpenError) as info:
            Image.open(str(tmp_path / "absent.png"))
        detail = info.value.imagemagick_detail
        assert detail is not None and "unable to open image" in detail

    def test_invalid_output_path_is_a_save_error(self, image: Image, tmp_path: Path) -> None:
        with pytest.raises(magik.MagikSaveError):
            image.save(str(tmp_path / "no-such-dir" / "out.png"))

    def test_unsupported_encoder_is_catchable_specifically(self, image: Image) -> None:
        try:
            image.to_bytes("XCF")
        except magik.MagikUnsupportedFormatError:
            pass  # the documented, specific outcome
        except magik.MagikError:
            pytest.skip("this ImageMagick build can write XCF")

    def test_errors_from_streams_are_not_swallowed(self, image: Image) -> None:
        class Broken:
            def write(self, data: bytes) -> int:
                raise OSError("disk full")

        with pytest.raises(OSError):
            image.save(Broken(), "PNG")


# ---------------------------------------------------------------------------
# §3 / §5 -- bytes-like inputs and outputs
# ---------------------------------------------------------------------------


class TestBytesLikeObjects:
    @pytest.mark.parametrize("wrap", [bytes, bytearray, memoryview])
    def test_from_bytes_accepts_any_buffer(self, png_bytes: bytes, wrap) -> None:
        assert Image.from_bytes(wrap(png_bytes)).size == (WIDTH, HEIGHT)

    def test_from_bytes_matches_open(self, png_bytes: bytes) -> None:
        assert Image.from_bytes(png_bytes).pixels() == Image.open(png_bytes).pixels()

    def test_to_bytes_returns_an_independent_bytes_object(self, image: Image) -> None:
        first = image.to_bytes("PNG")
        second = image.to_bytes("PNG")
        assert first == second
        assert first is not second
        # Mutating a mutable view of the result must not corrupt anything.
        view = bytearray(first)
        view[0] = 0
        assert Image.from_bytes(first).size == image.size

    def test_bytes_survive_the_source_image_being_dropped(self, image: Image) -> None:
        """The buffer must be copied out of ImageMagick before the wand dies."""
        data = image.to_bytes("PNG")
        del image
        assert Image.from_bytes(data).size == (WIDTH, HEIGHT)

    @pytest.mark.parametrize("fmt", ["PNG", "JPEG"])
    def test_to_bytes_then_from_bytes(self, image: Image, fmt: str, need_format) -> None:
        need_format(fmt)
        data = image.to_bytes(fmt)
        assert isinstance(data, bytes)
        assert Image.from_bytes(data).size == image.size


# ---------------------------------------------------------------------------
# §7 / §9 -- format versus mode, and metadata
# ---------------------------------------------------------------------------


class TestFormatVersusMode:
    def test_they_are_independent_attributes(self, image: Image) -> None:
        assert image.mode == "RGB"
        assert image.format == "PNG"

    @pytest.mark.parametrize("fmt", ["BMP", "TIFF"])
    @pytest.mark.parametrize("mode", ["RGB", "RGBA"])
    def test_containers_that_keep_the_type_preserve_the_mode(
        self, fmt: str, mode: str, tmp_path: Path, need_format
    ) -> None:
        """BMP and TIFF store the image type, so `mode` survives untouched.

        PNG is deliberately absent: ImageMagick decides there to re-encode as a
        palette, which changes `mode` even for a detailed image. That is covered
        separately, and it is ImageMagick's prerogative rather than a defect.
        """
        need_format(fmt)
        out = tmp_path / f"typed.{fmt.lower()}"
        detailed_image(mode).save(str(out))
        restored = Image.open(out)
        assert restored.mode == mode
        assert restored.pixels() == detailed_image(mode).pixels()

    def test_png_reclassifies_the_mode_but_never_the_pixels(self, tmp_path: Path) -> None:
        """`format` is authoritative for PNG; `mode` is ImageMagick's choice.

        Documenting both halves matters: a user who writes PNG and reads back
        `"P"` has not hit a magik bug, and the pixels they get are still right.
        """
        source = detailed_image("RGBA")
        out = tmp_path / "reclassified.png"
        source.save(str(out))
        restored = Image.open(out)

        assert restored.format == "PNG"
        assert restored.mode == "P", "ImageMagick reclassifies RGBA PNG to a palette"
        assert restored.pixel_mode == "RGBA", "...but the transparency is still transferred"
        assert restored.pixels() == source.pixels()

    def test_format_survives_a_lossy_round_trip_unchanged(self, image: Image, need_format) -> None:
        need_format("JPEG")
        assert Image.from_bytes(image.to_bytes("JPEG")).format == "JPEG"


class TestMetadataBasics:
    def test_the_documented_attributes_agree_with_each_other(self, image: Image) -> None:
        assert image.width == WIDTH
        assert image.height == HEIGHT
        assert image.size == (WIDTH, HEIGHT) == (image.width, image.height)
        assert image.mode == "RGB"
        assert image.format == "PNG"
        assert image.depth == 8

    def test_metadata_matches_the_properties(self, image: Image) -> None:
        meta = image.metadata()
        assert meta["width"] == image.width
        assert meta["height"] == image.height
        assert meta["mode"] == image.mode
        assert meta["format"] == image.format

    def test_a_new_image_reports_its_backing_format(self) -> None:
        """`Image.new` is MIFF-backed, so `format` is honest about that."""
        fresh = Image.new("RGB", (2, 2), "white")
        assert fresh.format == "MIFF"
        assert fresh.mode == "RGB"

    def test_fixtures_decode_with_the_expected_metadata(self, fixtures: Fixtures, need_format) -> None:
        for fmt in STAGE_03_FORMATS:
            need_format(fmt, fixtures)
            loaded = fixtures.image(fmt)
            assert loaded.size == (WIDTH, HEIGHT)
            expected = BMP_VARIANTS if fmt == "BMP" else (fmt,)
            assert loaded.format in expected, f"{fmt} decoded as {loaded.format}"
            assert loaded.format != loaded.mode, "format and mode must not coincide"


class TestModuleSurface:
    def test_package_level_open_matches_the_classmethod(self, fixtures: Fixtures) -> None:
        assert magik.open(fixtures.path("PNG")).format == Image.open(fixtures.path("PNG")).format

    def test_hand_built_png_decodes_without_imagemagick_helpers(self) -> None:
        """PNG fixtures are assembled from `zlib`, proving an independent read path."""
        raw = make_png_bytes()
        assert raw.startswith(b"\x89PNG\r\n\x1a\n")
        assert Image.from_bytes(raw).size == (WIDTH, HEIGHT)

    def test_documented_example_from_the_stage_specification(self, tmp_path: Path) -> None:
        """The exact flow the specification asks for, end to end."""
        source = tmp_path / "input.png"
        detailed_image("RGB", 32, 24).save(str(source))

        img = Image.open(source)
        assert img.size == (32, 24)

        img.save(str(tmp_path / "output.webp"))

        data = img.to_bytes("png")
        assert isinstance(data, bytes)

        img2 = Image.from_bytes(data)
        assert img2.size == (32, 24)
        assert img2.format == "PNG"
        assert img2.pixels() == img.pixels()