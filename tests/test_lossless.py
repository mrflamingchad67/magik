"""Data-preservation contract for the Stage 05 operations, at the Python surface.

Each test states whether it pins a *guarantee* (it follows from what the
operation is, so a failure is a magik bug) or an *observation* of the linked
ImageMagick build. Measured behaviour from ImageMagick 7.1.2-32 is labelled as
such and is never presented as a promise.
"""

from __future__ import annotations

import pytest

import magik
from magik import Image

W, H = 8, 8


def rgb() -> Image:
    """A wide-range RGB fixture, so lossy behaviour is visible."""
    return Image.new("RGB", (W, H), "black").putpixels(
        (0, 0),
        (W, H),
        bytes(
            v
            for i in range(W * H)
            for v in ((i * 29 + 7) % 256, (i * 53 + 11) % 256, (i * 17 + 3) % 256)
        ),
    )


def rgb_16bit() -> Image:
    """A 16-bit fixture with values on both sides of quantisation boundaries."""
    values = [0, 1, 255, 256, 32767, 32768, 65535]
    data = bytearray()
    for r in values:
        for g in values:
            for b in values:
                for v in (r, g, b):
                    data += bytes((v & 0xFF, v >> 8))
    count = len(values) ** 3
    return Image.from_pixels("RGB", (count, 1), bytes(data), depth=16)


def words(image: Image) -> list[int]:
    """16-bit transfer data as integers, rather than raw byte pairs."""
    raw = image.pixels(depth=16)
    return [int.from_bytes(raw[i : i + 2], "little") for i in range(0, len(raw), 2)]


# ---------------------------------------------------------------------------
# 1. has_alpha is read-only
# ---------------------------------------------------------------------------


class TestAlphaQueryIsReadOnly:
    def test_querying_alpha_changes_no_pixels(self) -> None:
        """A guarantee: this is a query, so any pixel change is a bug."""
        image = rgb()
        before = image.pixels()
        assert image.has_alpha is False
        assert image.pixels() == before

    def test_the_answer_is_stable(self) -> None:
        image = rgb()
        assert image.has_alpha == image.has_alpha

    def test_querying_does_not_alter_a_transparent_image(self) -> None:
        image = Image.new("RGBA", (W, H), (200, 100, 50, 128))
        before = image.pixels()
        assert image.has_alpha is True
        assert image.pixels() == before


# ---------------------------------------------------------------------------
# 2. extract_channel: exact values, not reversible
# ---------------------------------------------------------------------------


class TestChannelExtractionPreservesValues:
    @pytest.mark.parametrize("name", ["red", "green", "blue"])
    def test_each_channel_matches_the_source_exactly(self, name: str) -> None:
        """A guarantee: extraction selects a channel, so values must match."""
        image = rgb()
        channel = image.extract_channel(name)
        for y in range(H):
            for x in range(W):
                source = image.getpixel((x, y))[("red", "green", "blue").index(name)]
                assert channel.getpixel((x, y)) == source, f"{name} at ({x}, {y})"

    def test_extraction_is_a_projection_not_a_reversible_copy(self) -> None:
        """A guarantee: three channels cannot be recovered from one."""
        image = rgb()
        red = image.extract_channel("red")
        assert len(red.pixels()) < len(image.pixels())
        assert red.size == image.size

    def test_extracting_the_same_channel_twice_is_stable(self) -> None:
        image = rgb()
        assert (
            image.extract_channel("red").pixels()
            == image.extract_channel("red").pixels()
        )

    def test_alpha_extraction_is_exact_at_every_level(self) -> None:
        for alpha in (0, 1, 128, 254, 255):
            image = Image.new("RGBA", (W, H), (200, 100, 50, alpha))
            assert image.extract_channel("alpha").getpixel((0, 0)) == alpha


# ---------------------------------------------------------------------------
# 3. Colorspace identity
# ---------------------------------------------------------------------------


class TestColorspaceIdentity:
    def test_converting_to_the_active_colorspace_is_an_exact_no_op(self) -> None:
        """A guarantee: no transform requested, so samples must be untouched."""
        image = rgb()
        assert image.colorspace == "sRGB"
        assert image.convert_colorspace("sRGB").pixels() == image.pixels()

    def test_identity_holds_for_a_non_default_source(self) -> None:
        gray = Image.new("L", (W, H), 128)
        assert gray.convert_colorspace("Gray").pixels() == gray.pixels()


# ---------------------------------------------------------------------------
# 4. Round trips: 8-bit and 16-bit measured separately
# ---------------------------------------------------------------------------


class TestColorspaceRoundTrips:
    @pytest.mark.parametrize("space", ["Lab", "XYZ", "YCbCr", "HSL"])
    def test_eight_bit_invertible_round_trips_are_exact(self, space: str) -> None:
        """An OBSERVATION of ImageMagick 7.1.2-32, not a promise.

        Measured bit-exact on the build this was written against. A different
        ImageMagick build could differ by a unit; re-measure rather than assume
        magik regressed.
        """
        image = rgb()
        round_trip = image.convert_colorspace(space).convert_colorspace("sRGB")
        assert round_trip.pixels() == image.pixels(), f"{space} round trip at 8 bits"

    @pytest.mark.parametrize("space", ["Lab", "Luv", "XYZ", "YCbCr"])
    def test_sixteen_bit_round_trips_quantise_but_stay_close(self, space: str) -> None:
        """An OBSERVATION, and why 8-bit and 16-bit are tested separately.

        The transforms that are bit-exact at 8 bits lose precision at 16 because
        the intermediate representation rounds. Measured drift was at most 89 of
        65535 (~0.14%). The bound is deliberately far looser so it survives
        another build, while still catching a real precision regression such as
        truncation down to 8 bits.
        """
        image = rgb_16bit()
        round_trip = image.convert_colorspace(space).convert_colorspace("sRGB")
        before, after = words(image), words(round_trip)
        worst = max(abs(a - b) for a, b in zip(before, after))
        assert worst <= 65535 // 100, f"{space} drifted by {worst} of 65535"

    def test_sixteen_bit_identity_is_exact_whereas_a_transform_is_not(self) -> None:
        """The contrast that justifies the observation above.

        Returning to the same colorspace is structurally exact; going through Lab
        quantises. Measured on one build, but the *direction* of the difference is
        what this asserts.
        """
        image = rgb_16bit()
        identity = image.convert_colorspace("sRGB").convert_colorspace("sRGB")
        assert words(identity) == words(image)

        transformed = image.convert_colorspace("Lab").convert_colorspace("sRGB")
        assert words(transformed) != words(image)


# ---------------------------------------------------------------------------
# 5. Depth: widening and narrowing differ
# ---------------------------------------------------------------------------


class TestDepthPreservation:
    def test_widening_then_narrowing_preserves_every_sample(self) -> None:
        """A guarantee for widening: 8 bits are exact at 16, so 8->16->8 is identity."""
        image = rgb().convert_depth(8)
        before = image.pixels()
        assert image.convert_depth(16).convert_depth(8).pixels() == before

    @pytest.mark.parametrize("original", [1, 2, 255, 257, 4097])
    def test_narrowing_can_lose_information(self, original: int) -> None:
        """A guarantee: narrowing quantises and the low bits do not come back."""
        fine = Image.from_pixels("L", (1, 1), original.to_bytes(2, "little"), depth=16)
        assert fine.pixels(depth=16) == original.to_bytes(2, "little")

        widened = fine.convert_depth(8).convert_depth(16)
        result = words(widened)[0]

        if original == 1:
            assert result != original, (
                "narrowing to 8 bits must not pretend the original low byte survived"
            )
        # Whatever it became, it must be a genuine 8-bit-derived value: the high
        # byte repeats the low one, which is what "widened from 8" looks like.
        assert result >> 8 == result & 0xFF

    def test_depth_conversion_never_adds_colour_information(self) -> None:
        image = rgb().convert_depth(16).convert_depth(8)
        assert len(image.pixels()) == len(rgb().convert_depth(8).pixels())


# ---------------------------------------------------------------------------
# 6. Grayscale discards colour
# ---------------------------------------------------------------------------


class TestGrayscaleIsLossyByDesign:
    def test_a_coloured_image_does_not_survive_grayscale(self) -> None:
        """A guarantee: one channel cannot reproduce three."""
        coloured = Image.from_pixels("RGB", (1, 1), bytes((0, 0, 200)))
        gray = coloured.grayscale()
        round_trip = gray.convert_colorspace("sRGB")
        assert round_trip.pixels() != coloured.pixels()

    def test_grayscale_keeps_geometry(self) -> None:
        image = rgb()
        assert image.grayscale().size == image.size

    def test_this_is_designed_lossiness_not_a_bug(self) -> None:
        """Contrast an intentional collapse with an accidental one.

        The same image round-trips exactly through an invertible transform at 8
        bits, so a difference here is the operation working as documented rather
        than data being corrupted.
        """
        image = rgb()
        assert image.convert_colorspace("Lab").convert_colorspace("sRGB").pixels() == (
            image.pixels()
        )
        assert image.grayscale().convert_colorspace("sRGB").pixels() != image.pixels()


# ---------------------------------------------------------------------------
# 7. Alpha across transparency levels
# ---------------------------------------------------------------------------


class TestAlphaIsCarriedNotRecomputed:
    @pytest.mark.parametrize("alpha", [0, 1, 128, 254, 255])
    def test_conversion_leaves_the_alpha_sample_alone(self, alpha: int) -> None:
        """A guarantee: transparency is carried across, not recomputed."""
        image = Image.new("RGBA", (W, H), (200, 100, 50, alpha))
        assert image.has_alpha is True
        assert image.getpixel((0, 0))[3] == alpha

        converted = image.convert_colorspace("Gray")
        assert converted.has_alpha is True
        assert converted.getpixel((0, 0))[3] == alpha, "conversion changed the alpha sample"

    def test_an_opaque_image_gains_no_alpha(self) -> None:
        image = rgb()
        assert not image.has_alpha
        assert not image.convert_colorspace("Gray").has_alpha

    def test_extracting_alpha_does_not_leave_the_source_with_alpha(self) -> None:
        """The extracted alpha is opaque data, not an alpha channel."""
        image = Image.new("RGBA", (W, H), (200, 100, 50, 128))
        channel = image.extract_channel("alpha")
        assert channel.getpixel((0, 0)) == 128
        assert not channel.has_alpha
        assert image.has_alpha, "the source keeps its alpha"

    def test_transparency_survives_an_encode_and_reopen(self) -> None:
        image = Image.new("RGBA", (W, H), (200, 100, 50, 77))
        assert Image.from_bytes(image.to_bytes("MIFF")).has_alpha is True
        assert Image.from_bytes(image.to_bytes("MIFF")).getpixel((0, 0))[3] == 77