"""Stage 04: core operation contracts at the Python API surface.

`test_operations.py` covers the operation set broadly. This file goes after the
parts that are easy to get subtly wrong and hard to notice:

* **pixel-level correctness** against values computed by hand, not eyeballs;
* **mode and alpha behaviour** of `grayscale` and `blur`;
* the **dimension contract** of `rotate`, which differs by angle;
* interoperability with the Stage 03 I/O and Stage 02 pixel APIs.

Fixtures are deterministic: the value at `(x, y)` is a pure function of its own
coordinates, so any pixel after a transform can be predicted exactly.
"""

from __future__ import annotations

from pathlib import Path

import pytest

import magik
from magik import Image

W, H = 6, 4
CHANNELS = {"RGB": 3, "RGBA": 4, "L": 1}


def marker_value(x: int, y: int, channel: int) -> int:
    """The value the `marker` fixture stores at one coordinate."""
    return (x * 37 + y * 11 + channel * 53) % 256


def marker(mode: str, width: int = W, height: int = H) -> Image:
    """An asymmetric image whose every pixel is uniquely identifiable."""
    channels = CHANNELS[mode]
    data = bytes(
        marker_value(x, y, c)
        for y in range(height)
        for x in range(width)
        for c in range(channels)
    )
    fill = (0,) * channels
    return Image.new(mode, (width, height), fill).putpixels(
        (0, 0), (width, height), data
    )


def pixel(image: Image, x: int, y: int) -> tuple[int, ...]:
    """`getpixel` as a tuple, whatever shape the mode returns."""
    value = image.getpixel((x, y))
    return (value,) if isinstance(value, int) else tuple(value)


# ---------------------------------------------------------------------------
# §3.1 Resize
# ---------------------------------------------------------------------------


class TestResizeContract:
    @pytest.mark.parametrize("size", [(1, 1), (2, 3), (6, 4), (13, 7), (200, 150)])
    def test_exact_geometry(self, size: tuple[int, int]) -> None:
        assert marker("RGB").resize(*size).size == size

    def test_tuple_form_matches_positional(self) -> None:
        source = marker("RGB")
        assert source.resize((9, 3)).size == source.resize(9, 3).size

    def test_no_implicit_aspect_ratio(self) -> None:
        """magik resamples to an exact box; it never guesses a ratio."""
        source = marker("RGB")
        assert source.resize(20, 5).size == (20, 5)
        assert source.resize(5, 20).size == (5, 20)

    def test_deterministic(self) -> None:
        source = marker("RGB")
        assert source.resize(9, 7).pixels() == source.resize(9, 7).pixels()

    def test_filters_are_distinguishable(self) -> None:
        source = marker("RGB")
        point = source.resize(3, 2, filter="point").pixels()
        lanczos = source.resize(3, 2, filter="lanczos").pixels()
        assert point != lanczos, "the filter argument had no effect"

    def test_default_filter_is_lanczos(self) -> None:
        source = marker("RGB")
        assert source.resize(9, 7).pixels() == source.resize(9, 7, filter="lanczos").pixels()

    @pytest.mark.parametrize("size", [(0, 5), (5, 0), (0, 0), (-1, 5)])
    def test_invalid_dimensions_raise(self, size: tuple[int, int]) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").resize(*size)

    def test_missing_height_raises(self) -> None:
        with pytest.raises(TypeError):
            marker("RGB").resize(10)


# ---------------------------------------------------------------------------
# §3.2 Crop
# ---------------------------------------------------------------------------


class TestCropContract:
    def test_right_and_bottom_are_exclusive(self) -> None:
        assert marker("RGB").crop(0, 0, 3, 2).size == (3, 2)
        assert marker("RGB").crop(2, 1, 5, 4).size == (3, 3)

    def test_picks_up_exactly_the_expected_pixels(self) -> None:
        source = marker("RGB")
        cropped = source.crop(2, 1, 5, 4)
        for dx, dy in [(0, 0), (1, 0), (2, 0), (0, 1), (2, 2)]:
            assert pixel(cropped, dx, dy) == pixel(source, 2 + dx, 1 + dy), (dx, dy)

    @pytest.mark.parametrize(
        "box", [(0, 0, W, H), (0, 0, 1, 1), (W - 1, H - 1, W, H), (0, 0, 1, H)]
    )
    def test_boundary_boxes(self, box: tuple[int, int, int, int]) -> None:
        right, bottom = box[2], box[3]
        assert marker("RGB").crop(*box).size == (right - box[0], bottom - box[1])

    def test_tuple_form(self) -> None:
        assert marker("RGB").crop((1, 1, 4, 3)).size == marker("RGB").crop(1, 1, 4, 3).size

    @pytest.mark.parametrize(
        "box", [(0, 0, 0, 4), (3, 0, 3, 4), (0, 0, 6, 0), (4, 4, 2, 2)]
    )
    def test_degenerate_boxes_raise(self, box: tuple[int, int, int, int]) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").crop(*box)

    @pytest.mark.parametrize("box", [(0, 0, W + 1, H), (0, 0, W, H + 1), (0, 0, 99, 99)])
    def test_out_of_bounds_raises_rather_than_padding(self, box) -> None:
        """A documented divergence from Pillow, which pads with black."""
        with pytest.raises(magik.MagikOperationError) as info:
            marker("RGB").crop(*box)
        assert "exceeds image bounds" in str(info.value)

    @pytest.mark.parametrize("box", [(-1, 0, 3, 3), (0, -1, 3, 3)])
    def test_negative_origin_raises(self, box) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").crop(*box)

    def test_repeated_cropping_narrows(self) -> None:
        source = marker("RGB")
        once = source.crop(0, 0, 4, 3)
        twice = once.crop(1, 1, 3, 2)
        assert twice.size == (2, 1)
        assert pixel(twice, 0, 0) == pixel(source, 1, 1)
        assert source.size == (W, H)

    def test_missing_arguments_raise(self) -> None:
        with pytest.raises(TypeError):
            marker("RGB").crop(0, 0, 2)


# ---------------------------------------------------------------------------
# §3.3 Rotate
# ---------------------------------------------------------------------------


class TestRotateContract:
    @pytest.mark.parametrize(("angle", "size"), [(0, (W, H)), (180, (W, H)), (90, (H, W)), (270, (H, W))])
    def test_right_angles_transpose(self, angle: int, size: tuple[int, int]) -> None:
        assert marker("RGB").rotate(angle).size == size

    def test_positive_angles_are_counter_clockwise(self) -> None:
        """A counter-clockwise quarter turn maps source (x, y) to (y, W-1-x)."""
        source = marker("RGB")
        rotated = source.rotate(90)
        assert pixel(rotated, 0, 0) == pixel(source, W - 1, 0), "top-right moves to top-left"
        assert pixel(rotated, 0, H - 1) == pixel(source, W - 1 - (H - 1), 0)

    def test_negative_angles_are_clockwise(self) -> None:
        source = marker("RGB")
        rotated = source.rotate(-90)
        assert pixel(rotated, 0, 0) == pixel(source, 0, H - 1), "bottom-left moves to top-left"

    def test_zero_and_full_turns_are_identity(self) -> None:
        source = marker("RGB")
        assert source.rotate(0).pixels() == source.pixels()
        assert source.rotate(360).pixels() == source.pixels()

    def test_four_quarter_turns_restore_the_image(self) -> None:
        source = marker("RGB")
        result = source
        for _ in range(4):
            result = result.rotate(90)
        assert result.size == source.size
        assert result.pixels() == source.pixels()

    def test_half_turn_is_a_point_reflection(self) -> None:
        source = marker("RGB")
        turned = source.rotate(180)
        for x, y in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)]:
            assert pixel(turned, x, y) == pixel(source, W - 1 - x, H - 1 - y), (x, y)

    def test_oblique_angle_grows_the_canvas(self) -> None:
        """ImageMagick expands so the rotated corners are not clipped.

        An earlier doc comment claimed the opposite; the growth is real and is
        asserted here so the documentation and the code cannot drift again.
        """
        rotated = marker("RGB").rotate(45)
        assert rotated.width > W
        assert rotated.height > H

    def test_default_background_is_transparent(self) -> None:
        corner = pixel(marker("RGB").rotate(45), 0, 0)
        assert len(corner) == 4, "an exposed corner carries alpha"
        assert corner[3] == 0

    def test_explicit_background_fills_the_corner(self) -> None:
        assert pixel(marker("RGB").rotate(45, background="white"), 0, 0) == (255, 255, 255)

    def test_background_is_keyword_only(self) -> None:
        with pytest.raises(TypeError):
            marker("RGB").rotate(45, "white")

    @pytest.mark.parametrize("angle", [float("nan"), float("inf"), float("-inf")])
    def test_non_finite_angles_raise(self, angle: float) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").rotate(angle)

    def test_unparseable_background_raises(self) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").rotate(45, background="definitely-not-a-colour")


# ---------------------------------------------------------------------------
# §3.4 Grayscale
# ---------------------------------------------------------------------------


class TestGrayscaleContract:
    def test_rgb_becomes_single_channel(self) -> None:
        gray = marker("RGB").grayscale()
        assert gray.mode == "L"
        assert gray.pixel_mode == "L"
        assert len(gray.pixels()) == W * H

    def test_size_is_preserved(self) -> None:
        assert marker("RGB").grayscale().size == (W, H)

    def test_alpha_is_preserved_not_dropped(self) -> None:
        """A see-through pixel must stay see-through."""
        source = marker("RGBA")
        gray = source.grayscale()
        assert gray.mode == "LA", "the mode string reports gray plus transparency"
        assert pixel(gray, 0, 0)[3] == pixel(source, 0, 0)[3], "alpha must survive"

    def test_gray_channels_are_replicated(self) -> None:
        value = pixel(marker("RGBA").grayscale(), 0, 0)
        assert value[0] == value[1] == value[2]

    def test_opaque_rgba_stays_opaque(self) -> None:
        opaque = Image.new("RGBA", (4, 4), (255, 0, 0, 255)).grayscale()
        assert pixel(opaque, 0, 0)[3] == 255

    def test_idempotent(self) -> None:
        once = marker("RGB").grayscale()
        assert once.grayscale().pixels() == once.pixels()

    def test_grayscale_input_is_stable(self) -> None:
        source = Image.new("L", (4, 4), 128)
        assert source.grayscale().mode == "L"
        assert source.grayscale().size == (4, 4)

    def test_luminance_ordering_is_preserved(self) -> None:
        """White stays brighter than black; the ramp must not clip."""
        ramp = Image.new("L", (64, 1), 0).putpixels((0, 0), (64, 1), bytes(range(0, 256, 4)))
        gray = ramp.grayscale().pixels()
        assert gray[0] < gray[32]
        assert gray[63] < 255


# ---------------------------------------------------------------------------
# §3.5 Blur
# ---------------------------------------------------------------------------


class TestBlurContract:
    @pytest.mark.parametrize(("radius", "sigma"), [(1, 0.5), (2, 1), (0.5, 0.25), (6, 3)])
    def test_geometry_is_preserved(self, radius: float, sigma: float) -> None:
        assert marker("RGB").blur(radius, sigma).size == (W, H)

    def test_sigma_defaults_to_half_the_radius(self) -> None:
        source = marker("RGB")
        assert source.blur(4).pixels() == source.blur(4, 2).pixels()

    def test_zero_radius_is_the_identity(self) -> None:
        source = marker("RGB")
        assert source.blur(0, 0).pixels() == source.pixels()

    def test_blur_actually_smooths(self) -> None:
        """A hard vertical edge must gain intermediate values.

        The far column legitimately stays saturated - a radius-2 kernel does not
        reach it - so the signature of smoothing is the appearance of greys
        *between* the two flat regions, not a global reduction in contrast.
        """
        row = b"".join(bytes((0, 0, 0)) * (W // 2) + bytes((255, 255, 255)) * (W // 2) for _ in range(H))
        edge = Image.new("RGB", (W, H), "black").putpixels((0, 0), (W, H), row)
        blurred = edge.blur(2, 1).pixels()

        middle_row = blurred[(H // 2) * W * 3 : ((H // 2) + 1) * W * 3]
        red = middle_row[0::3]
        # Blurring a hard black/white edge must produce a monotonic ramp across
        # it. The outermost columns stay saturated, which is correct - a
        # radius-2 kernel does not reach them.
        assert len(set(red)) > 2, f"the edge was not smoothed: {list(red)}"
        assert red[0] == 0 and red[-1] == 255, f"endpoints moved: {list(red)}"
        assert list(red) == sorted(red), f"the ramp is not monotonic: {list(red)}"
        assert red[W // 2 - 1] > 0, "the dark side should have picked up light"

    def test_uniform_alpha_is_unchanged(self) -> None:
        source = Image.new("RGBA", (W, H), (10, 20, 30, 200))
        assert pixel(source.blur(2, 1), 0, 0)[3] == 200

    def test_blur_applies_to_alpha(self) -> None:
        """Documented: every channel is blurred, matching Pillow."""
        source = marker("RGBA")
        before = [pixel(source, x, y)[3] for y in range(H) for x in range(W)]
        after = [pixel(source.blur(2, 1), x, y)[3] for y in range(H) for x in range(W)]
        assert before != after, "a varying alpha field must be blurred too"

    def test_single_pixel_image_is_fine(self) -> None:
        dot = Image.new("RGB", (1, 1), (200, 100, 50))
        assert dot.blur(2, 1).size == (1, 1)
        assert pixel(dot.blur(2, 1), 0, 0) == (200, 100, 50)

    @pytest.mark.parametrize(
        ("radius", "sigma"),
        [(-1, 1), (1, -1), (-1, -1), (float("nan"), 1), (1, float("nan")), (float("inf"), 1)],
    )
    def test_invalid_parameters_raise(self, radius: float, sigma: float) -> None:
        with pytest.raises(magik.MagikOperationError):
            marker("RGB").blur(radius, sigma)

    def test_non_numeric_radius_raises(self) -> None:
        with pytest.raises(TypeError):
            marker("RGB").blur("wide")


# ---------------------------------------------------------------------------
# §3.6 Flip and flop
# ---------------------------------------------------------------------------


class TestMirrorContract:
    def test_flip_is_vertical(self) -> None:
        source = marker("RGB")
        flipped = source.flip()
        assert pixel(flipped, 0, 0) == pixel(source, 0, H - 1)
        assert flipped.size == (W, H)

    def test_flop_is_horizontal(self) -> None:
        source = marker("RGB")
        flopped = source.flop()
        assert pixel(flopped, 0, 0) == pixel(source, W - 1, 0)
        assert flopped.size == (W, H)

    def test_they_are_different_operations(self) -> None:
        source = marker("RGB")
        assert source.flip().pixels() != source.flop().pixels()

    def test_every_pixel_of_flip(self) -> None:
        source = marker("RGB")
        flipped = source.flip()
        for y in range(H):
            for x in range(W):
                assert pixel(flipped, x, y) == pixel(source, x, H - 1 - y), (x, y)

    def test_every_pixel_of_flop(self) -> None:
        source = marker("RGB")
        flopped = source.flop()
        for y in range(H):
            for x in range(W):
                assert pixel(flopped, x, y) == pixel(source, W - 1 - x, y), (x, y)

    def test_double_application_is_identity(self) -> None:
        source = marker("RGB")
        assert source.flip().flip().pixels() == source.pixels()
        assert source.flop().flop().pixels() == source.pixels()

    def test_flip_and_flop_commute(self) -> None:
        source = marker("RGB")
        assert source.flip().flop().pixels() == source.flop().flip().pixels()

    def test_four_quarter_turns_equal_two_mirrors(self) -> None:
        source = marker("RGB")
        assert source.rotate(180).pixels() == source.flip().flop().pixels()

    def test_transparent_pixels_travel_with_the_image(self) -> None:
        source = marker("RGBA")
        flipped = source.flip()
        for x, y in [(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)]:
            assert pixel(flipped, x, H - 1 - y) == pixel(source, x, y), (x, y)

    @pytest.mark.parametrize("size", [(4, 1), (1, 4), (1, 1)])
    def test_degenerate_shapes(self, size: tuple[int, int]) -> None:
        source = Image.new("RGB", size, "black")
        assert source.flip().size == size
        assert source.flop().size == size


# ---------------------------------------------------------------------------
# §4 / §6 Shared contracts, chaining, and Stage 03 interoperability
# ---------------------------------------------------------------------------


class TestOperationContracts:
    @pytest.fixture
    def source(self) -> Image:
        return marker("RGB", 100, 80)

    def test_no_operation_mutates_its_input(self, source: Image) -> None:
        before = source.pixels()
        for result in (
            source.resize(10, 10),
            source.crop(1, 1, 20, 20),
            source.rotate(45),
            source.rotate(90),
            source.grayscale(),
            source.blur(2, 1),
            source.flip(),
            source.flop(),
        ):
            assert result.size[0] > 0 and result.size[1] > 0
        assert source.pixels() == before
        assert source.size == (100, 80)

    def test_results_are_independent(self, source: Image) -> None:
        small = source.resize(10, 10)
        large = source.resize(90, 70)
        assert small.size == (10, 10)
        assert large.size == (90, 70), "a later call resized an earlier result"

    def test_documented_chain(self, source: Image) -> None:
        """The chain from the specification, adapted to the real fixtures."""
        original_size = source.size
        result = (
            source.resize(80, 60)
            .crop(5, 5, 75, 55)
            .grayscale()
            .blur(1, 0.5)
            .rotate(90)
        )
        assert result.size == (50, 70)
        assert source.size == original_size

    def test_chain_survives_save_and_reopen(self, source: Image, tmp_path: Path) -> None:
        result = source.resize(40, 30).crop(2, 2, 30, 22).grayscale()
        out = tmp_path / "chain.png"
        result.save(str(out))
        reopened = Image.open(out)
        assert reopened.size == (28, 20)
        assert reopened.pixels() == result.pixels()

    @pytest.mark.parametrize("fmt", ["PNG", "TIFF", "BMP"])
    def test_lossless_formats_round_trip_operations(self, source: Image, fmt: str, need_format) -> None:
        need_format(fmt)
        operated = source.resize(40, 30).crop(5, 5, 35, 25).flip()
        assert Image.from_bytes(operated.to_bytes(fmt)).pixels() == operated.pixels()

    @pytest.mark.parametrize("mode", ["RGB", "RGBA", "L"])
    def test_operations_behave_consistently_across_modes(self, mode: str) -> None:
        image = Image.new(mode, (W, H), 128 if mode == "L" else tuple([128] * CHANNELS[mode]))
        assert image.resize(3, 3).size == (3, 3)
        assert image.rotate(90).size == (H, W)
        assert image.crop(0, 0, 2, 2).size == (2, 2)
        assert image.flip().size == (W, H)
        assert image.flop().size == (W, H)
        assert image.blur(1, 0.5).size == (W, H)
        assert image.size == (W, H), "the source was mutated"

    def test_grayscale_then_blur_is_valid(self) -> None:
        result = marker("RGB").grayscale().blur(1, 0.5)
        assert result.mode == "L"
        assert result.size == (W, H)

    def test_rotate_after_resize_uses_the_new_size(self) -> None:
        """Ordering is respected: rotation sees the resized geometry."""
        assert marker("RGB").resize(10, 4).rotate(90).size == (4, 10)

    def test_crop_after_rotate_uses_the_rotated_geometry(self) -> None:
        assert marker("RGB").rotate(90).crop(0, 0, 2, 2).size == (2, 2)

    def test_pixel_api_reads_operation_results(self, source: Image) -> None:
        """Stage 02 pixel access reads Stage 04 results unchanged."""
        operated = source.crop(10, 10, 50, 50)
        assert len(operated.pixels()) == 40 * 40 * 3
        assert pixel(operated, 0, 0) == pixel(source, 10, 10)

    def test_sixteen_bit_results_are_readable(self, source: Image) -> None:
        operated = source.grayscale()
        width, height = operated.size
        assert len(operated.pixels(depth=16)) == width * height * 2

    def test_a_failed_operation_leaves_the_image_usable(self, source: Image) -> None:
        """A rejected call must not corrupt the receiver."""
        with pytest.raises(magik.MagikOperationError):
            source.crop(0, 0, 9999, 9999)
        assert source.size == (100, 80)
        assert source.resize(50, 40).size == (50, 40)
        assert pixel(source, 0, 0) == (0, 53, 106)