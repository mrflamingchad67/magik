"""Stage 01 baseline benchmarks.

Purpose
-------

This is a **baseline**, not a performance demonstration. Stage 01 deliberately
performs no optimisation; these numbers exist so that later stages can measure
whether a change actually helped, and so the cost of the immutable
copy-on-write model is on the record rather than guessed at.

Nothing here is a microbenchmark of a hot loop: every measurement is a complete
Python-facing call, including argument marshalling and error handling, because
that is what a caller actually pays.

Usage
-----

.. code-block:: console

    python benches/benchmark.py                     # default 20 iterations
    python benches/benchmark.py --iterations 100
    python benches/benchmark.py --only resize
    python benches/benchmark.py --json results.json

Methodology
-----------

* Identical, deterministic inputs for every run (generated in-process, cached on
  disk next to this file so runs are comparable).
* A warm-up pass per scenario, discarded, so first-call page faults and lazily
  loaded coder modules are not counted.
* `time.perf_counter` around a whole batch, divided by the iteration count:
  individual calls are too short to time reliably.
* Best-of-N repeats, reported as the minimum, which is the least noisy estimator
  for this kind of measurement.
* No assertion or threshold: this records, it does not gate.
"""

from __future__ import annotations

import argparse
import json
import platform
import statistics
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tests"))

import magik  # noqa: E402
from magik import Image  # noqa: E402
from magik_fixtures import build  # noqa: E402

#: Sizes of the generated benchmark inputs.
SIZES = {"small": (256, 256), "medium": (1024, 768), "large": (2560, 1600)}

#: Sizes used by the pixel benchmarks. Stage 02 requires 512x512, 1080p and 4K.
PIXEL_SIZES = {"hd": (512, 512), "fhd": (1920, 1080), "uhd": (3840, 2160)}

#: Sizes used by the I/O benchmarks (Stage 03).
IO_SIZES = {"sd": (640, 480), "hd": (1280, 720), "fhd": (1920, 1080)}

#: Formats the I/O benchmarks cover. Each is probed before use, so a build
#: without a delegate reports nothing for it rather than failing.
IO_FORMATS = ("PNG", "JPEG", "WEBP")

#: Where the generated inputs are cached between runs.
INPUT_DIR = Path(__file__).resolve().parent / "inputs"


@dataclass
class Result:
    """One scenario's measurements."""

    name: str
    unit: str
    samples: list[float] = field(default_factory=list)

    @property
    def best(self) -> float:
        return min(self.samples)

    @property
    def median(self) -> float:
        return statistics.median(self.samples)

    @property
    def mean(self) -> float:
        return statistics.fmean(self.samples)

    @property
    def stdev(self) -> float:
        return statistics.pstdev(self.samples) if len(self.samples) > 1 else 0.0

    @property
    def ops_per_second(self) -> float:
        """Throughput implied by the best sample."""
        if self.best <= 0:
            return float("inf")
        divisor = 1_000.0 if self.unit == "ms" else 1_000_000.0
        return divisor / self.best

    def row(self) -> str:
        return (
            f"{self.name:<34} {self.best:>10.3f} {self.median:>10.3f} "
            f"{self.stdev:>9.3f} {self.ops_per_second:>10.1f}"
        )


def prepare_inputs(force: bool = False) -> dict[str, Path]:
    """Generate (once) and return the benchmark input files."""
    if force or not any(INPUT_DIR.glob("*.png")):
        build(INPUT_DIR)
    return {name: INPUT_DIR / "sample.png" for name in SIZES}


def scale(image: Image, size: tuple[int, int]) -> Image:
    """Return `image` at exactly `size`."""
    return image.resize(size[0], size[1])


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def scenarios(source: Path) -> list[tuple[str, str, Callable[[], None]]]:
    """Build the (name, unit, callable) triples to measure."""
    base = Image.open(source)
    medium = scale(base, SIZES["medium"])
    small = scale(base, SIZES["small"])
    encoded = medium.to_bytes("PNG")

    out_dir = INPUT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    png_out = out_dir / "bench-out.png"
    jpg_out = out_dir / "bench-out.jpg"

    return [
        # -- loading --------------------------------------------------------
        ("open.png.small", "ms", lambda: Image.open(source)),
        (
            "open.png.medium_from_bytes",
            "ms",
            lambda: Image.open(scale(base, SIZES["small"]).to_bytes("PNG")),
        ),
        ("decode.png.medium_from_bytes", "ms", lambda: Image.open(encoded)),
        ("open.path.medium_every_call", "ms", lambda: Image.open(source)),

        # -- operations -----------------------------------------------------
        ("resize.small_from_medium", "ms", lambda: medium.resize(256, 256)),
        ("resize.medium_from_small_up", "ms", lambda: small.resize(1024, 768)),
        ("crop.medium_half", "ms", lambda: medium.crop(0, 0, 512, 384)),
        ("grayscale.medium", "ms", lambda: medium.grayscale()),
        ("blur.medium_small_radius", "ms", lambda: medium.blur(2, 1)),
        ("flip.medium", "ms", lambda: medium.flip()),
        ("flop.medium", "ms", lambda: medium.flop()),
        ("rotate.medium_90", "ms", lambda: medium.rotate(90)),
        ("copy.medium", "ms", lambda: medium.copy()),
        ("metadata.medium", "us", lambda: medium.metadata()),

        # -- saving ---------------------------------------------------------
        ("encode.png.medium", "ms", lambda: medium.to_bytes("PNG")),
        ("encode.jpeg.medium", "ms", lambda: medium.to_bytes("JPEG")),
        ("save.png.medium_to_path", "ms", lambda: medium.save(str(png_out))),
        ("save.jpeg.medium_to_path", "ms", lambda: medium.save(str(jpg_out))),
    ]


def measure(
    name: str,
    unit: str,
    body: Callable[[], None],
    iterations: int,
    repeats: int,
    warmup: int,
) -> Result:
    """Time `body`, returning the result in `unit`."""
    scale_factor = 1000.0 if unit == "ms" else 1_000_000.0

    for _ in range(warmup):
        body()

    samples: list[float] = []
    for _ in range(repeats):
        start = time.perf_counter()
        for _ in range(iterations):
            body()
        elapsed = time.perf_counter() - start
        samples.append(elapsed / iterations * scale_factor)
    return Result(name=name, unit=unit, samples=samples)


# ---------------------------------------------------------------------------
# Stage 02: pixel access
# ---------------------------------------------------------------------------


def pixel_scenarios() -> list[tuple[str, str, Callable[[], None]]]:
    """Single-pixel and bulk pixel scenarios.

    The point of measuring both is the comparison itself: repeated
    ``getpixel`` calls cross the Python boundary and make one ImageMagick call
    each, whereas ``pixels()`` is a single call returning one ``bytes`` object.
    The gap is the argument for the bulk API existing at all.
    """
    scenarios: list[tuple[str, str, Callable[[], None]]] = []
    for label, (width, height) in PIXEL_SIZES.items():
        rgb = Image.new("RGB", (width, height), "black")
        gray = Image.new("L", (width, height), "black")

        # A single sample near the middle: the same coordinate every time, so
        # the cache state is comparable between the two accessors.
        mid = (width // 2, height // 2)

        scenarios.append((f"getpixel.rgb.{label}", "us", lambda i=rgb, p=mid: i.getpixel(p)))
        scenarios.append((f"getpixel.gray.{label}", "us", lambda i=gray, p=mid: i.getpixel(p)))
        scenarios.append((f"putpixel.rgb.{label}", "us", lambda i=rgb, p=mid: i.putpixel(p, (1, 2, 3))))
        scenarios.append((f"pixels.rgb.{label}", "ms", lambda i=rgb: i.pixels()))
        scenarios.append((f"pixels.gray.{label}", "ms", lambda i=gray: i.pixels()))
        scenarios.append((f"pixels16.rgb.{label}", "ms", lambda i=rgb: i.pixels(depth=16)))
        scenarios.append((f"new.rgb.{label}", "ms", lambda s=(width, height): Image.new("RGB", s, "black")))
        scenarios.append(
            (
                f"from_pixels.rgb.{label}",
                "ms",
                lambda s=(width, height), d=bytes(width * height * 3): Image.from_pixels("RGB", s, d),
            )
        )
    return scenarios


# ---------------------------------------------------------------------------
# Stage 03: image I/O
# ---------------------------------------------------------------------------


def writable_format(fmt: str, probe: Image) -> bool:
    """Whether this ImageMagick build can actually encode `fmt`."""
    try:
        probe.to_bytes(fmt)
    except magik.MagikError:
        return False
    return True


def io_scenarios() -> list[tuple[str, str, Callable[[], None]]]:
    """Load and save scenarios across sizes and formats.

    Each operation is measured twice: once against a **path** and once against
    an in-memory **buffer** that holds exactly the same bytes. The two differ
    only in whether the filesystem is touched, so the gap between them
    approximates the disk cost while the buffer-only number approximates pure
    ImageMagick work. That is how the I/O and processing shares are separated
    here — by differencing two real measurements rather than by adding a
    syscall-level probe that would not reflect how the library is used.

    Encoded inputs and outputs go to a temporary directory, never into the
    repository.
    """
    scenarios: list[tuple[str, str, Callable[[], None]]] = []
    scratch = Path(tempfile.mkdtemp(prefix="magik-bench-io-"))

    for label, (width, height) in IO_SIZES.items():
        # A synthetic photo-like source: real gradients compress unrealistically
        # well for some coders, which would flatter PNG and distort the numbers.
        ramp = bytearray()
        for y in range(height):
            for x in range(width):
                ramp += bytes(
                    (
                        (x * 7 + y * 3) % 256,
                        (x * 5 + y * 11) % 256,
                        (x + y * 13) % 256,
                    )
                )
        image = Image.new("RGB", (width, height), "black").putpixels(
            (0, 0), (width, height), bytes(ramp)
        )

        for fmt in IO_FORMATS:
            if not writable_format(fmt, image):
                continue
            try:
                encoded = image.to_bytes(fmt)
            except magik.MagikError:
                continue
            suffix = {"JPEG": "jpg"}.get(fmt, fmt.lower())
            on_disk = scratch / f"{label}.{suffix}"
            on_disk.write_bytes(encoded)

            scenarios.append(
                (f"io.open_path.{fmt.lower()}.{label}", "ms", lambda p=on_disk: Image.open(p))
            )
            scenarios.append(
                (f"io.open_bytes.{fmt.lower()}.{label}", "ms", lambda b=encoded: Image.open(b))
            )
            scenarios.append(
                (
                    f"io.save_path.{fmt.lower()}.{label}",
                    "ms",
                    lambda i=image, p=on_disk: i.save(str(p)),
                )
            )
            scenarios.append(
                (f"io.to_bytes.{fmt.lower()}.{label}", "ms", lambda i=image, f=fmt: i.to_bytes(f))
            )
            scenarios.append(
                (f"io.from_bytes.{fmt.lower()}.{label}", "ms", lambda b=encoded: Image.from_bytes(b))
            )
    return scenarios


def io_rows(results: list[Result]) -> list[str]:
    """Split each measurement into a filesystem share and a processing share."""
    by_name = {r.name: r for r in results}
    lines: list[str] = []
    for label, (width, height) in IO_SIZES.items():
        for fmt in IO_FORMATS:
            tag = f"{fmt.lower()}.{label}"
            path_load = by_name.get(f"io.open_path.{tag}")
            buf_load = by_name.get(f"io.open_bytes.{tag}")
            path_save = by_name.get(f"io.save_path.{tag}")
            buf_save = by_name.get(f"io.to_bytes.{tag}")
            if not all([path_load, buf_load, path_save, buf_save]):
                continue

            megapixels = width * height / 1e6
            load_disk = max(path_load.best - buf_load.best, 0.0)
            save_disk = max(path_save.best - buf_save.best, 0.0)
            lines.append(
                f"  {tag:<12} {width}x{height} ({megapixels:4.1f} MP)  "
                f"load {path_load.best:8.2f} ms total = {buf_load.best:8.2f} ms decode "
                f"+ {load_disk:6.2f} ms fs   |   "
                f"save {path_save.best:8.2f} ms total = {buf_save.best:8.2f} ms encode "
                f"+ {save_disk:6.2f} ms fs"
            )
    if lines:
        lines.append("")
        lines.append(
            "  The two halves are differenced from separate measurements, not isolated:"
        )
        lines.append(
            "  the 'decode'/'encode' figure still includes the Python<->Rust copy, and the"
        )
        lines.append(
            "  'fs' figure also absorbs any per-call setup the path route adds."
        )
    return lines


def comparison_rows(results: list[Result]) -> list[str]:
    """Derive the bulk-vs-single-pixel ratios from the measured results."""
    by_name = {r.name: r for r in results}
    lines: list[str] = []
    for label, (width, height) in PIXEL_SIZES.items():
        count = width * height
        single = by_name.get(f"getpixel.rgb.{label}")
        bulk = by_name.get(f"pixels.rgb.{label}")
        if not (single and bulk):
            continue
        # `bulk.best` is milliseconds; convert to microseconds, then per pixel.
        bulk_us_per_pixel = bulk.best * 1000.0 / count
        speedup = single.best / bulk_us_per_pixel
        lines.append(
            f"  {label:<4} {width}x{height} ({count:,} px): "
            f"1x getpixel = {single.best:8.2f} us  |  "
            f"pixels() = {bulk.best:8.2f} ms = {bulk_us_per_pixel:6.3f} us/px  |  "
            f"bulk is {speedup:7.0f}x cheaper per pixel"
        )
        single_put = by_name.get(f"putpixel.rgb.{label}")
        bulk_build = by_name.get(f"from_pixels.rgb.{label}")
        if single_put and bulk_build:
            build_us_per_pixel = bulk_build.best * 1000.0 / count
            # Deliberately NOT dividing these two into a speed-up ratio. Unlike
            # reads, they are not comparable per pixel: `putpixel` spends its time
            # cloning the whole wand (magik's immutability), not writing a pixel,
            # so a ratio here would compare copy-on-write against bulk import and
            # say nothing about pixel cost.
            lines.append(
                f"  {'':<4} {'writes':<22} 1x putpixel = {single_put.best:8.2f} us "
                f"(dominated by the copy-on-write clone, not by the pixel write)  |  "
                f"from_pixels() = {bulk_build.best:8.2f} ms "
                f"= {build_us_per_pixel:6.3f} us/px"
            )
    return lines


def environment() -> dict[str, str]:
    """A short description of the machine and build, recorded with results."""
    im = magik.version()
    return {
        "magik": magik.__version__,
        "imagemagick": im["version"],
        "quantum_depth": im["quantum_depth"],
        "python": platform.python_version(),
        "platform": platform.platform(),
        "machine": platform.machine(),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--iterations", type=int, default=20, help="calls per sample")
    parser.add_argument("--repeats", type=int, default=3, help="samples per scenario")
    parser.add_argument("--warmup", type=int, default=3, help="discarded warm-up calls")
    parser.add_argument("--only", default=None, help="run scenarios whose name contains this")
    parser.add_argument("--json", type=Path, default=None, help="also write results as JSON")
    parser.add_argument("--regenerate", action="store_true", help="rebuild the inputs")
    parser.add_argument(
        "--pixels",
        action="store_true",
        help="run only the Stage 02 pixel scenarios (skips the Stage 01 ones)",
    )
    parser.add_argument(
        "--io",
        action="store_true",
        help="run only the Stage 03 image I/O scenarios",
    )
    args = parser.parse_args(argv)

    selected: list[tuple[str, str, Callable[[], None]]] = []
    if args.pixels:
        selected += pixel_scenarios()
    elif args.io:
        selected += io_scenarios()
    else:
        source = next(iter(prepare_inputs(args.regenerate).values()))
        selected += scenarios(source)
        selected += pixel_scenarios()

    if args.only:
        selected = [scenario for scenario in selected if args.only in scenario[0]]
    if not selected:
        print(f"no scenario matched {args.only!r}", file=sys.stderr)
        return 2

    env = environment()
    print("magik baseline benchmarks")
    print(f"  magik        {env['magik']}")
    print(f"  ImageMagick  {env['imagemagick']}")
    print(f"  python       {env['python']} on {env['platform']}")
    print(
        f"  {args.iterations} iterations x {args.repeats} samples "
        f"(+{args.warmup} warm-up), best of samples reported\n"
    )
    header = f"{'scenario':<34} {'best':>10} {'median':>10} {'stdev':>9} {'ops/s':>10}"
    print(header)
    print("-" * len(header))
    print("(times are in the unit named by the scenario: ms or us)\n")

    results: list[Result] = []
    for name, unit, body in selected:
        result = measure(name, unit, body, args.iterations, args.repeats, args.warmup)
        results.append(result)
        print(result.row())

    rows = comparison_rows(results)
    if rows:
        print("\nbulk vs single-pixel (derived from the numbers above):")
        for line in rows:
            print(line)

    io_lines = io_rows(results)
    if io_lines:
        print("\nI/O split (derived from the numbers above):")
        for line in io_lines:
            print(line)

    if args.json:
        payload = {
            "environment": env,
            "iterations": args.iterations,
            "repeats": args.repeats,
            "results": [
                {
                    "name": r.name,
                    "unit": r.unit,
                    "best": r.best,
                    "median": r.median,
                    "mean": r.mean,
                    "stdev": r.stdev,
                }
                for r in results
            ],
        }
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        print(f"\nwrote {args.json}")

    print(
        "\nThese are Stage 01/02 baselines, not targets. No optimisation has been\n"
        "attempted; use them to judge later changes."
    )
    return 0

    env = environment()
    print("magik baseline benchmarks")
    print(f"  magik        {env['magik']}")
    print(f"  ImageMagick  {env['imagemagick']}")
    print(f"  python       {env['python']} on {env['platform']}")
    print(
        f"  {args.iterations} iterations x {args.repeats} samples "
        f"(+{args.warmup} warm-up), best of samples reported\n"
    )
    header = f"{'scenario':<34} {'best':>10} {'median':>10} {'stdev':>9} {'ops/s':>10}"
    print(header)
    print("-" * len(header))
    print("(times are in the unit named by the scenario: ms or us)\n")

    results: list[Result] = []
    for name, unit, body in selected:
        result = measure(name, unit, body, args.iterations, args.repeats, args.warmup)
        results.append(result)
        print(result.row())

    if args.json:
        payload = {
            "environment": env,
            "iterations": args.iterations,
            "repeats": args.repeats,
            "results": [
                {
                    "name": r.name,
                    "unit": r.unit,
                    "best": r.best,
                    "median": r.median,
                    "mean": r.mean,
                    "stdev": r.stdev,
                }
                for r in results
            ],
        }
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        print(f"\nwrote {args.json}")

    print(
        "\nThese are Stage 01 baselines, not targets. No optimisation has been\n"
        "attempted; use them to judge later changes."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())