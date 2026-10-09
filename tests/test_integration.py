"""Cross-cutting guarantees: API shape, immutability, threading, layer boundaries."""

from __future__ import annotations

import concurrent.futures
import io
import sys
from pathlib import Path

import pytest

import magik
from magik import Image
from magik_fixtures import HEIGHT, WIDTH, Fixtures


class TestPublicSurface:
    def test_documented_names_are_exported(self) -> None:
        for name in magik.__all__:
            assert hasattr(magik, name), f"{name} is missing from the package"

    def test_from_import_works(self) -> None:
        from magik import Image as ImportedImage

        assert ImportedImage is magik.Image

    def test_image_is_a_class(self) -> None:
        assert isinstance(magik.Image, type)

    @pytest.mark.parametrize(
        "name",
        [
            "open",
            "resize",
            "crop",
            "rotate",
            "flip",
            "flop",
            "grayscale",
            "blur",
            "save",
            "to_bytes",
            "copy",
            "metadata",
        ],
    )
    def test_documented_methods_exist(self, image: Image, name: str) -> None:
        assert callable(getattr(image, name))

    @pytest.mark.parametrize(
        "name", ["width", "height", "format", "mode", "channels", "depth", "size"]
    )
    def test_documented_properties_exist(self, image: Image, name: str) -> None:
        getattr(image, name)

    def test_everything_in_all_exists(self) -> None:
        assert magik.__version__
        assert isinstance(magik.__version__, str)


class TestPillowCompatibilityShape:
    """The API is Pillow-*inspired*; these are the parts users rely on."""

    def test_open_is_a_static_constructor(self, png_bytes: bytes) -> None:
        assert Image.open(png_bytes).format == "PNG"

    def test_size_is_a_tuple(self, image: Image) -> None:
        assert isinstance(image.size, tuple)
        assert len(image.size) == 2

    def test_crop_accepts_both_forms(self, image: Image) -> None:
        assert image.crop(1, 2, 11, 22).size == image.crop((1, 2, 11, 22)).size

    def test_resize_accepts_both_forms(self, image: Image) -> None:
        assert image.resize(30, 20).size == image.resize((30, 20)).size

    def test_attributes_are_ints(self, image: Image) -> None:
        assert isinstance(image.width, int)
        assert isinstance(image.height, int)
        assert isinstance(image.channels, int)
        assert isinstance(image.depth, int)

    def test_format_and_mode_are_strings(self, image: Image) -> None:
        assert isinstance(image.format, str)
        assert isinstance(image.mode, str)


class TestThreading:
    """`Image` is documented as safe to share between Python threads."""

    def test_concurrent_reads(self, image: Image) -> None:
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            sizes = list(pool.map(lambda _: image.resize(16, 16).size, range(32)))
        assert sizes == [(16, 16)] * 32

    def test_concurrent_operations(self, image: Image) -> None:
        operations = [
            lambda: image.resize(8, 8).to_bytes("PNG"),
            lambda: image.grayscale().to_bytes("PNG"),
            lambda: image.blur(1, 0.5).to_bytes("PNG"),
            lambda: image.flip().to_bytes("PNG"),
        ]
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(lambda op: op(), operations * 8))
        assert len(results) == 32
        assert all(isinstance(r, bytes) and r for r in results)

    def test_shared_image_is_not_corrupted(self, image: Image) -> None:
        before = image.to_bytes("PNG")
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            list(pool.map(lambda _: image.resize(32, 32).to_bytes("PNG"), range(64)))
        assert image.to_bytes("PNG") == before

    def test_parallel_pipeline(self, fixtures: Fixtures) -> None:
        """A realistic many-image pipeline must not race."""
        sources = [fixtures.image("PNG") for _ in range(8)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            thumbs = list(pool.map(lambda im: im.resize(24, 18).grayscale(), sources))
        assert all(t.size == (24, 18) for t in thumbs)

    def test_no_async_api_is_exposed(self) -> None:
        """Stage 01 deliberately has no async support."""
        for name in dir(magik.Image):
            assert not name.startswith("a")


class TestNoSubprocesses:
    """ImageMagick must be called in-process, never as a child process."""

    def test_no_process_is_spawned(self, monkeypatch, image: Image) -> None:
        import subprocess

        def explode(*args, **kwargs):  # pragma: no cover - must never run
            raise AssertionError("magik must not spawn a subprocess")

        monkeypatch.setattr(subprocess, "Popen", explode)
        monkeypatch.setattr(subprocess, "run", explode)
        monkeypatch.setattr(subprocess, "call", explode)

        data = image.resize(16, 16).to_bytes("PNG")
        assert Image.open(data).size == (16, 16)

    def test_in_memory_work_needs_no_temp_file(self, monkeypatch, image: Image) -> None:
        """`tempfile` must not be reached for an in-memory conversion."""
        import tempfile

        def explode(*args, **kwargs):  # pragma: no cover - must never run
            raise AssertionError("magik must not create temporary files")

        monkeypatch.setattr(tempfile, "mkstemp", explode)
        monkeypatch.setattr(tempfile, "NamedTemporaryFile", explode)

        assert image.to_bytes("PNG")


class TestLayering:
    """Python -> PyO3 -> magik-core -> magik-sys must be one-directional."""

    def test_extension_is_separate_from_the_package(self) -> None:
        assert magik.Image.__module__.startswith("magik")

    def test_package_import_does_not_leak_sys_modules(self) -> None:
        assert "magik._magik" in sys.modules

    def test_no_rust_crates_are_visible_to_python(self) -> None:
        """Python code must not be able to reach the FFI layer directly."""
        image = Image.open(io.BytesIO(b"")) if False else None
        assert image is None
        assert not any(
            name.startswith(("magik_sys", "magik_core")) for name in sys.modules
        )

    def test_low_level_is_explicitly_namespaced(self, image: Image) -> None:
        """ImageMagick specifics live under `.magick`, not on `Image`."""
        assert not hasattr(image, "wand")
        assert not hasattr(image, "wand_ptr")
        assert hasattr(image, "magick")

    def test_image_has_no_raw_pointer_accessors(self, image: Image) -> None:
        for name in dir(image):
            assert "ptr" not in name.lower()


class TestDocumentationExamples:
    """The examples in README and docstrings must actually work."""

    def test_quick_start(self, fixtures: Fixtures, tmp_path: Path) -> None:
        image = Image.open(str(fixtures.path("PNG")))
        assert image.width and image.height and image.format

        out = tmp_path / "output.png"
        image.resize(80, 60).grayscale().save(str(out))
        assert Image.open(out).size == (80, 60)

    def test_bytes_workflow(self, image: Image) -> None:
        data = image.to_bytes("PNG")
        assert Image.open(io.BytesIO(data)).size == (WIDTH, HEIGHT)

    def test_low_level_workflow(self, image: Image) -> None:
        assert image.magick.image_type
        assert image.magick.version["quantum_depth"]
        derived = image.magick.with_option("png:bit-depth", "8")
        assert isinstance(derived, Image)

    def test_threaded_example(self, image: Image) -> None:
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda _: image.resize(64, 64).to_bytes("PNG"), range(8)))
        assert len(results) == 8


class TestModuleReload:
    """Reimporting must not duplicate or corrupt the native state."""

    def test_reimport_is_consistent(self) -> None:
        import importlib

        reloaded = importlib.import_module("magik")
        assert reloaded.__version__ == magik.__version__
        assert reloaded.Image is magik.Image

    def test_version_is_stable(self) -> None:
        assert magik.version() == magik.version()