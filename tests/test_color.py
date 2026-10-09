"""Stage 05: colorspaces, channels, alpha and precision at the Python surface.

Expected values come from measured ImageMagick behaviour, not assumption. The
distinction that matters most here is *conversion* against *assignment*: both
accept "Gray", and they return different numbers, because only one of them
recomputes anything.
"""

from __future__ import annotations

from pathlib import Path

import pytest

import magik
from magik import Image

W, H = 4, 4


def distinct_rgb() -> Image:
    """RGB whose channels all differ, so a mis-mapped channel is visible."""
    return Image.new("RGB", (W, H), "black").putpixels(
        (0, 0), (W, H), bytes(v for _ in range(W * H) for v in (200, 100, 50))
    )


def rgba(alpha: int) -> Image:
    """RGBA with an exact alpha sample.

    Uses the tuple form: `ImageMagick parses ``rgba()`` alpha as a 0..1 fraction,
    so the string form is the wrong tool for an absolute sample.
    """
    return Image.new("RGBA", (W, H), (200, 100, 50, alpha))


def blue_pixel() -> Image:
    """A single ``(0, 0, 200)`` pixel: luminance 14, red channel 0."""
    return Image.new("RGB", (1, 1), "black").putpixels((0, 0), (1, 1), bytes((0, 0, 200)))


def first(image: Image) -> int:
    """Top-left sample of a single-channel image."""
    value = image.getpixel((0, 0))
    return value if isinstance(value, int) else value[0]


# ---------------------------------------------------------------------------
# Requirement A - conversion versus assignment
# ---------------------------------------------------------------------------


class TestColorspaceConversion:
    def test_conversion_recomputes_the_pixel_values(self) -> None:
        assert first(blue_pixel().convert_colorspace("Gray")) == 14

    def test_assignment_leaves_the_samples_alone(self) -> None:
        """The contrast that makes `convert_colorspace` worth having.

        Treating ``(0, 0, 200)`` *as* Gray does not turn it into a luminance; the
        first sample is still the red channel, which is ``0``.
        """
        assert first(blue_pixel().magick.with_colorspace("Gray")) == 0

    def test_conversion_and_assignment_disagree(self) -> None:
        image = blue_pixel()
        converted = image.convert_colorspace("Gray")
        assigned = image.magick.with_colorspace("Gray")
        assert converted.pixels() != assigned.pixels()
        assert first(converted) != first(assigned)

    def test_grayscale_is_conversion_to_gray(self) -> None:
        image = blue_pixel()
        assert image.grayscale().pixels() == image.convert_colorspace("Gray").pixels()

    @pytest.mark.parametrize("name", ["Gray", "sRGB", "CMYK", "HSL", "Lab", "RGB"])
    def test_conversion_preserves_dimensions(self, name: str) -> None:
        assert distinct_rgb().convert_colorspace(name).size == (W, H)

    def test_conversion_is_case_insensitive(self) -> None:
        image = distinct_rgb()
        assert (
            image.convert_colorspace("GRAY").pixels()
            == image.convert_colorspace("gray").pixels()
        )

    @pytest.mark.parametrize("alias", ["Gray", "grey", "l"])
    def test_conversion_accepts_documented_aliases(self, alias: str) -> None:
        assert distinct_rgb().convert_colorspace(alias).size == (W, H)

    @pytest.mark.parametrize("bad", ["NotAColorspace", "GrayAlpha", "", "   "])
    def test_unknown_colorspaces_are_rejected(self, bad: str) -> None:
        with pytest.raises(magik.MagikFormatError):
            distinct_rgb().convert_colorspace(bad)

    def test_source_is_immutable(self) -> None:
        image = distinct_rgb()
        before = image.pixels()
        for name in ("Gray", "CMYK", "HSL"):
            image.convert_colorspace(name)
        assert image.pixels() == before
        assert image.size == (W, H)
        assert image.colorspace == "sRGB"

    def test_conversion_is_deterministic(self) -> None:
        image = distinct_rgb()
        assert (
            image.convert_colorspace("Gray").pixels()
            == image.convert_colorspace("Gray").pixels()
        )


class TestConversionAlpha:
    def test_alpha_survives_conversion(self) -> None:
        converted = rgba(128).convert_colorspace("Gray")
        assert converted.has_alpha
        assert converted.getpixel((0, 0))[3] == 128, "alpha must not be recomputed"

    def test_opaque_image_does_not_gain_alpha(self) -> None:
        converted = distinct_rgb().convert_colorspace("Gray")
        assert not converted.has_alpha
        assert converted.mode == "L"

    def test_gray_alpha_reports_four_samples(self) -> None:
        """Stage 04's documented caveat still holds; this stage must not change it."""
        converted = rgba(128).convert_colorspace("Gray")
        assert converted.mode == "LA"
        assert converted.pixel_mode == "RGBA"
        assert len(converted.getpixel((0, 0))) == 4


class TestColorspaceRoundTrip:
    def test_a_gray_round_trip_is_lossy(self) -> None:
        """Collapsing to one channel and back cannot restore the original.

        Asserting the loss is deliberate: a round trip through Gray is not
        promised to be an identity, and pretending otherwise would be a lie
        about a lossy operation.
        """
        original = blue_pixel()
        round_trip = original.convert_colorspace("Gray").convert_colorspace("sRGB")
        assert round_trip.pixels() != original.pixels()

    def test_round_trip_stays_close_for_smooth_colours(self) -> None:
        """A grey survives the trip, and stays single-channel.

        Two things are asserted: the sample is preserved, and widening back to
        `sRGB` does **not** invent colour channels. ImageMagick keeps the image
        grayscale and only relabels the colorspace, which is why `getpixel`
        returns a scalar rather than a triple here.
        """
        grey = Image.new("RGB", (2, 2), (128, 128, 128))
        round_trip = grey.convert_colorspace("Gray").convert_colorspace("sRGB")
        assert round_trip.mode == "L", "Gray -> sRGB must not add colour channels"
        assert round_trip.colorspace == "sRGB"
        value = round_trip.getpixel((0, 0))
        assert isinstance(value, int)
        assert abs(value - 128) <= 2, "the grey level should survive, within rounding"


# ---------------------------------------------------------------------------
# Requirement B - channel extraction
# ---------------------------------------------------------------------------


class TestChannelExtraction:
    @pytest.mark.parametrize(
        ("name", "expected"), [("red", 200), ("green", 100), ("blue", 50)]
    )
    def test_rgb_channels_are_extracted_exactly(self, name: str, expected: int) -> None:
        channel = distinct_rgb().extract_channel(name)
        assert first(channel) == expected
        assert channel.size == (W, H)
        assert channel.mode == "L"
        assert channel.pixel_mode == "L"

    def test_alpha_channel_is_extracted_exactly(self) -> None:
        channel = rgba(128).extract_channel("alpha")
        assert first(channel) == 128
        assert channel.size == (W, H)

    @pytest.mark.parametrize("alpha", [0, 1, 64, 200, 255])
    def test_alpha_extraction_is_exact_for_every_level(self, alpha: int) -> None:
        assert first(rgba(alpha).extract_channel("alpha")) == alpha

    def test_channels_of_an_rgba_image(self) -> None:
        image = rgba(128)
        assert first(image.extract_channel("red")) == 200
        assert first(image.extract_channel("green")) == 100
        assert first(image.extract_channel("blue")) == 50
        assert first(image.extract_channel("alpha")) == 128

    def test_every_pixel_is_extracted_not_just_the_first(self) -> None:
        """A non-uniform fixture, so a one-pixel implementation cannot pass."""
        image = Image.new("RGB", (4, 4), "black").putpixels(
            (0, 0),
            (4, 4),
            bytes(v for y in range(4) for x in range(4) for v in (x * 10, y * 10, 7)),
        )
        red = image.extract_channel("red")
        assert [red.getpixel((x, y)) for y in range(4) for x in range(4)] == [
            x * 10 for y in range(4) for x in range(4)
        ]

    def test_alpha_extraction_from_an_opaque_image_is_an_error(self) -> None:
        with pytest.raises(magik.MagikOperationError) as info:
            distinct_rgb().extract_channel("alpha")
        assert "alpha" in str(info.value)

    def test_colour_extraction_from_grayscale_is_an_error(self) -> None:
        gray = Image.new("L", (W, H), 128)
        for name in ("red", "green", "blue"):
            with pytest.raises(magik.MagikOperationError):
                gray.extract_channel(name)

    def test_cmyk_channels_are_available_and_rgb_is_not(self) -> None:
        cmyk = Image.new("CMYK", (W, H), (0, 0, 0, 0))
        for name in ("cyan", "magenta", "yellow", "black"):
            channel = cmyk.extract_channel(name)
            assert channel.size == (W, H)
            assert channel.pixel_mode == "L"
        with pytest.raises(magik.MagikOperationError) as info:
            cmyk.extract_channel("red")
        assert "cyan" in str(info.value)

    @pytest.mark.parametrize("bad", ["notachannel", "", "   ", "luma", "gray", "REDD"])
    def test_unknown_channels_are_rejected(self, bad: str) -> None:
        with pytest.raises(magik.MagikFormatError):
            distinct_rgb().extract_channel(bad)

    def test_channel_names_are_case_insensitive(self) -> None:
        image = distinct_rgb()
        assert (
            image.extract_channel("RED").pixels() == image.extract_channel("red").pixels()
        )

    def test_extraction_does_not_mutate_the_source(self) -> None:
        image = distinct_rgb()
        before = image.pixels()
        for name in ("red", "green", "blue"):
            image.extract_channel(name)
        assert image.pixels() == before
        assert image.size == (W, H)
        assert not image.has_alpha, "extraction must not add alpha to the source"

    def test_extraction_does_not_change_the_sources_colorspace(self) -> None:
        image = distinct_rgb()
        before = image.colorspace
        image.extract_channel("red")
        assert image.colorspace == before

    def test_extraction_is_repeatable(self) -> None:
        image = distinct_rgb()
        assert (
            image.extract_channel("green").pixels()
            == image.extract_channel("green").pixels()
        )

    def test_extracted_channel_round_trips_through_png(self, tmp_path: Path) -> None:
        channel = distinct_rgb().extract_channel("red")
        out = tmp_path / "red.png"
        channel.save(str(out))
        reopened = Image.open(out)
        assert reopened.size == (W, H)
        assert first(reopened) == 200

    def test_extraction_interoperates_with_operations(self) -> None:
        channel = distinct_rgb().extract_channel("green")
        assert channel.resize(8, 8).size == (8, 8)
        assert channel.blur(1, 0.5).size == (W, H)


# ---------------------------------------------------------------------------
# Requirement C - alpha introspection
# ---------------------------------------------------------------------------


class TestAlphaIntrospection:
    @pytest.mark.parametrize(
        ("image_factory", "expected"),
        [
            (lambda: Image.new("RGB", (W, H), (1, 2, 3)), False),
            (lambda: rgba(255), True),
            (lambda: rgba(128), True),
            (lambda: rgba(0), True),
            (lambda: Image.new("L", (W, H), 128), False),
            (lambda: Image.new("CMYK", (W, H), (0, 0, 0, 0)), False),
        ],
    )
    def test_alpha_presence(self, image_factory, expected: bool) -> None:
        assert image_factory().has_alpha is expected

    def test_alpha_is_not_inferred_from_the_channel_count(self) -> None:
        """CMYK has four channels and no alpha.

        Any implementation guessing from `channels` reports ``True`` here, which
        is why this is pinned explicitly.
        """
        cmyk = Image.new("CMYK", (W, H), (0, 0, 0, 0))
        assert cmyk.channels == 4
        assert cmyk.has_alpha is False

    def test_alpha_survives_an_encode_and_reopen(self) -> None:
        assert Image.from_bytes(rgba(128).to_bytes("MIFF")).has_alpha is True

    def test_opaque_image_stays_without_alpha_through_io(self) -> None:
        assert Image.from_bytes(distinct_rgb().to_bytes("MIFF")).has_alpha is False

    def test_has_alpha_is_a_property(self) -> None:
        assert isinstance(rgba(128).has_alpha, bool)

    def test_alpha_survives_grayscale_per_stage_04_contract(self) -> None:
        """Stage 04 promised grayscale() preserves alpha; this stage must not break it."""
        gray = rgba(128).grayscale()
        assert gray.has_alpha is True
        assert gray.getpixel((0, 0))[3] == 128


# ---------------------------------------------------------------------------
# Requirement D - depth and precision
# ---------------------------------------------------------------------------


class TestDepthConversion:
    def test_depth_conversion_reports_the_new_depth(self) -> None:
        image = distinct_rgb()
        assert image.convert_depth(8).depth == 8
        assert image.convert_depth(16).depth == 16

    def test_reducing_depth_is_lossy(self) -> None:
        """The central honesty test: 16 -> 8 -> 16 must not claim to recover data.

        A 16-bit sample of ``1`` is ``0x0001``. At 8 bits it rounds to ``1``, and
        widening that back gives ``0x0101`` - the original detail is gone and no
        amount of nominal precision brings it back.
        """
        fine = Image.from_pixels("L", (1, 1), bytes([0x00, 0x01]), depth=16)
        assert fine.pixels(depth=16) == bytes([0x00, 0x01])

        widened = fine.convert_depth(8).convert_depth(16)
        assert widened.pixels(depth=16) == bytes([0x01, 0x01])
        assert widened.pixels(depth=16) != fine.pixels(depth=16)

    def test_widening_then_narrowing_is_stable(self) -> None:
        """8 -> 16 -> 8 is the identity; going up must not change the samples."""
        image = Image.new("L", (W, H), "black").putpixels(
            (0, 0), (W, H), bytes((i * 17) % 256 for i in range(W * H))
        )
        before = image.convert_depth(8).pixels()
        after = image.convert_depth(8).convert_depth(16).convert_depth(8).pixels()
        assert after == before

    @pytest.mark.parametrize("bad", [0, 1, 7, 24, 32, 64])
    def test_unsupported_depths_are_rejected(self, bad: int) -> None:
        with pytest.raises(magik.MagikFormatError) as info:
            distinct_rgb().convert_depth(bad)
        assert "8 and 16" in str(info.value)

    def test_depth_conversion_is_immutable(self) -> None:
        image = distinct_rgb()
        before = image.pixels()
        image.convert_depth(8)
        image.convert_depth(16)
        assert image.pixels() == before

    def test_transfer_depth_is_independent_of_image_depth(self) -> None:
        """`depth=` on `pixels()` and `image.depth` are different concepts."""
        image = Image.from_pixels("RGB", (W, H), bytes(W * H * 3 * 2), depth=16)
        assert len(image.pixels(depth=8)) == W * H * 3
        assert len(image.pixels(depth=16)) == W * H * 3 * 2

    def test_result_survives_save_and_reopen(self, tmp_path: Path) -> None:
        converted = distinct_rgb().convert_depth(8)
        out = tmp_path / "depth.png"
        converted.save(str(out))
        reopened = Image.open(out)
        assert reopened.size == (W, H)
        assert reopened.pixels() == converted.pixels()

    def test_existing_pixel_apis_still_reject_unsupported_depths(self) -> None:
        with pytest.raises(magik.MagikFormatError):
            distinct_rgb().pixels(depth=32)

    def test_existing_from_pixels_buffer_rules_are_unchanged(self) -> None:
        with pytest.raises(magik.MagikOperationError):
            Image.from_pixels("RGB", (W, H), bytes(W * H * 3 + 1))


# ---------------------------------------------------------------------------
# Cross-cutting contracts
# ---------------------------------------------------------------------------


class TestStage05Contracts:
    def test_every_new_operation_returns_a_usable_image(self) -> None:
        image = distinct_rgb()
        for result in (
            image.convert_colorspace("Gray"),
            image.extract_channel("red"),
            image.convert_depth(16),
        ):
            assert result.size == (W, H)
            assert result.getpixel((0, 0)) is not None
            assert result.pixels()
            assert result.metadata()["format"]
            assert isinstance(result.to_bytes("PNG"), bytes)

    def test_operations_chain(self) -> None:
        result = (
            distinct_rgb()
            .convert_colorspace("RGB")
            .extract_channel("red")
            .convert_depth(16)
            .blur(1, 0.5)
        )
        assert result.size == (W, H)

    def test_grayscale_still_works_alongside_conversion(self) -> None:
        image = distinct_rgb()
        assert image.grayscale().mode == "L"
        assert image.convert_colorspace("Gray").mode == "L"

    def test_documented_example_from_the_specification(self, tmp_path: Path) -> None:
        """The flow a user would actually write."""
        image = distinct_rgb()

        gray = image.convert_colorspace("Gray")
        assert gray.mode == "L"

        red = image.extract_channel("red")
        assert first(red) == 200

        assert image.has_alpha is False

        wide = image.convert_depth(16)
        assert wide.depth == 16

        out = tmp_path / "red.png"
        red.save(str(out))
        assert Image.open(out).size == (W, H)