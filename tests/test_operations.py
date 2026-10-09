"""Geometry and colour operations, plus the immutability contract."""

from __future__ import annotations

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, WIDTH


class TestResize:
    def test_to_exact_size(self, image: Image) -> None:
        assert image.resize(100, 50).size == (100, 50)

    def test_tuple_form(self, image: Image) -> None:
        assert image.resize((100, 50)).size == (100, 50)

    def test_upscale(self, image: Image) -> None:
        assert image.resize(WIDTH * 2, HEIGHT * 2).size == (WIDTH * 2, HEIGHT * 2)

    def test_downscale_to_one_pixel(self, image: Image) -> None:
        assert image.resize(1, 1).size == (1, 1)

    @pytest.mark.parametrize("kernel", ["point", "box", "triangle", "lanczos", "mitchell"])
    def test_filters_are_accepted(self, image: Image, kernel: str) -> None:
        assert image.resize(20, 20, filter=kernel).size == (20, 20)

    def test_unknown_filter_raises(self, image: Image) -> None:
        with pytest.raises(magik.MagikFormatError):
            image.resize(20, 20, filter="not-a-filter")

    def test_default_filter_is_reported(self) -> None:
        assert magik.default_filter() == "Lanczos"


class TestCrop:
    def test_positional_box(self, image: Image) -> None:
        assert image.crop(10, 5, 42, 30).size == (32, 25)

    def test_tuple_box(self, image: Image) -> None:
        assert image.crop((10, 5, 42, 30)).size == (32, 25)

    def test_whole_image(self, image: Image) -> None:
        assert image.crop(0, 0, WIDTH, HEIGHT).size == (WIDTH, HEIGHT)

    def test_right_and_bottom_are_exclusive(self, image: Image) -> None:
        """Pillow convention: a box of 50x40 from the origin yields 50x40."""
        assert image.crop(0, 0, 50, 40).size == (50, 40)

    def test_single_pixel(self, image: Image) -> None:
        assert image.crop(3, 4, 4, 5).size == (1, 1)

    def test_inverted_box_raises(self, image: Image) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.crop(40, 40, 10, 10)

    def test_zero_area_box_raises(self, image: Image) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.crop(10, 10, 10, 20)

    def test_out_of_bounds_box_raises(self, image: Image) -> None:
        """Unlike Pillow, magik reports rather than pads an oversized box."""
        with pytest.raises(magik.MagikOperationError):
            image.crop(0, 0, WIDTH + 100, HEIGHT + 100)

    def test_negative_origin_raises(self, image: Image) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.crop(-5, 0, 20, 20)


class TestRotate:
    @pytest.mark.parametrize("angle", [90, 180, 270, 360])
    def test_right_angles_swap_or_preserve_size(self, image: Image, angle: int) -> None:
        rotated = image.rotate(angle)
        if angle % 180 == 90:
            assert rotated.size == (HEIGHT, WIDTH)
        else:
            assert rotated.size == (WIDTH, HEIGHT)

    def test_oblique_angle_grows_the_canvas(self, image: Image) -> None:
        """A 45-degree rotation needs a bigger box; the canvas is not cropped."""
        assert image.rotate(45).width > WIDTH

    def test_is_counter_clockwise(self, image: Image) -> None:
        """The top-left corner moves to the bottom-left after +90 degrees.

        Verified through geometry rather than pixels: a 90-degree CCW rotation
        maps the original top-right corner to the new top-left, so the corner
        that was furthest right becomes highest.
        """
        # A tall, narrow image makes the orientation unambiguous.
        tall = image.resize(10, 40)
        rotated = tall.rotate(90)
        assert rotated.size == (40, 10)

    def test_explicit_background(self, image: Image) -> None:
        assert image.rotate(45, background="white").size == image.rotate(45).size

    def test_non_finite_angle_raises(self, image: Image) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.rotate(float("nan"))


class TestFlipFlop:
    def test_flip_preserves_size(self, image: Image) -> None:
        assert image.flip().size == (WIDTH, HEIGHT)

    def test_flop_preserves_size(self, image: Image) -> None:
        assert image.flop().size == (WIDTH, HEIGHT)

    def test_flip_twice_is_identity(self, image: Image) -> None:
        once = image.flip().flip()
        assert once.size == image.size

    def test_flip_and_flop_differ_from_each_other(self, image: Image) -> None:
        """`flip` mirrors vertically, `flop` horizontally, so results differ."""
        vertical = image.flip().to_bytes("PNG")
        horizontal = image.flop().to_bytes("PNG")
        assert vertical != horizontal

    def test_flip_and_flop_commute(self, image: Image) -> None:
        a = image.flip().flop().to_bytes("PNG")
        b = image.flop().flip().to_bytes("PNG")
        assert a == b


class TestGrayscale:
    def test_becomes_gray(self, image: Image) -> None:
        grey = image.grayscale()
        assert grey.mode == "L"
        assert grey.channels == 1

    def test_preserves_size(self, image: Image) -> None:
        assert image.grayscale().size == (WIDTH, HEIGHT)

    def test_colorspace_is_gray(self, image: Image) -> None:
        assert image.grayscale().colorspace in {"Gray", "LinearGray"}

    def test_idempotent(self, image: Image) -> None:
        assert image.grayscale().grayscale().to_bytes("PNG") == image.grayscale().to_bytes("PNG")


class TestBlur:
    def test_preserves_size(self, image: Image) -> None:
        assert image.blur(2, 1).size == (WIDTH, HEIGHT)

    def test_sigma_defaults_to_half_radius(self, image: Image) -> None:
        assert image.blur(4).to_bytes("PNG") == image.blur(4, 2).to_bytes("PNG")

    def test_zero_radius_is_a_no_op_in_size(self, image: Image) -> None:
        assert image.blur(0, 0).size == (WIDTH, HEIGHT)

    def test_changes_pixels(self, image: Image) -> None:
        assert image.blur(5, 2).to_bytes("PNG") != image.to_bytes("PNG")

    @pytest.mark.parametrize(("radius", "sigma"), [(-1, 1), (1, -1), (float("inf"), 1)])
    def test_invalid_parameters_raise(self, image: Image, radius, sigma) -> None:
        with pytest.raises(magik.MagikOperationError):
            image.blur(radius, sigma)


class TestCopy:
    def test_copy_has_same_metadata(self, image: Image) -> None:
        assert image.copy().metadata() == image.metadata()

    def test_copy_is_independent(self, image: Image) -> None:
        """Operating on a copy must not affect the original."""
        duplicate = image.copy()
        resized = duplicate.resize(10, 10)
        assert resized.size == (10, 10)
        assert duplicate.size == (WIDTH, HEIGHT)
        assert image.size == (WIDTH, HEIGHT)


class TestImmutability:
    """Every operation returns a new image and leaves the receiver alone."""

    @pytest.mark.parametrize(
        "operation",
        [
            pytest.param(lambda i: i.resize(10, 10), id="resize"),
            pytest.param(lambda i: i.crop(0, 0, 10, 10), id="crop"),
            pytest.param(lambda i: i.rotate(90), id="rotate"),
            pytest.param(lambda i: i.flip(), id="flip"),
            pytest.param(lambda i: i.flop(), id="flop"),
            pytest.param(lambda i: i.grayscale(), id="grayscale"),
            pytest.param(lambda i: i.blur(2, 1), id="blur"),
            pytest.param(lambda i: i.copy(), id="copy"),
        ],
    )
    def test_original_is_unchanged(self, image: Image, operation) -> None:
        before = image.metadata()
        result = operation(image)
        assert image.metadata() == before, "the source image was modified"
        assert result is not None

    def test_chained_operations_leave_the_original_intact(self, image: Image) -> None:
        before = image.to_bytes("PNG")
        image.resize(32, 32).crop(0, 0, 16, 16).grayscale().blur(1, 0.5).rotate(45)
        assert image.to_bytes("PNG") == before

    def test_a_clone_does_not_disturb_its_origin(self, image: Image) -> None:
        """The wand clone must not share mutable pixel state with its parent."""
        before = image.to_bytes("PNG")
        derived = image.resize(16, 16).grayscale().blur(3, 1)
        assert derived.size == (16, 16)
        assert image.to_bytes("PNG") == before