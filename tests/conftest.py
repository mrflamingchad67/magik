"""Shared pytest fixtures for the magik test-suite."""

from __future__ import annotations

import pytest

import magik
from magik_fixtures import HEIGHT, MINIMAL_GIF, WIDTH, Fixtures, build


@pytest.fixture(scope="session")
def fixtures(tmp_path_factory: pytest.TempPathFactory) -> Fixtures:
    """A deterministic fixture set, generated once for the whole session."""
    return build(tmp_path_factory.mktemp("magik-images"))


@pytest.fixture
def png_bytes() -> bytes:
    """A hand-assembled 64x48 truecolour PNG."""
    from magik_fixtures import make_png_bytes

    return make_png_bytes()


@pytest.fixture
def image(png_bytes: bytes) -> magik.Image:
    """An open 64x48 RGB image."""
    return magik.Image.open(png_bytes)


@pytest.fixture
def tiny_gif() -> magik.Image:
    """A 1x1 image decoded from hand-written GIF89a bytes."""
    return magik.Image.open(MINIMAL_GIF)


@pytest.fixture
def need_format():
    """Return a callable that skips the test when a format is unavailable.

    Usage::

        def test_x(need_format, fixtures):
            need_format("WEBP", fixtures)
            ...
    """

    def _need(fmt: str, fixtures: Fixtures | None = None) -> None:
        from magik_fixtures import skip_reason

        reason = skip_reason(fmt, fixtures)
        if reason:
            pytest.skip(reason)

    return _need


__all__ = ["HEIGHT", "WIDTH"]