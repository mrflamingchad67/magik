# magik

A **Pillow-inspired, ImageMagick-powered** image library for Python, with a
Rust core and a PyO3 extension.

magik gives you the familiar Python ergonomics of Pillow — `Image.open(...)`,
`image.resize(...)`, `image.save(...)` — on top of the full breadth of
ImageMagick's codecs, while a safe Rust layer owns every native resource and a
deliberately scoped low-level namespace exposes the ImageMagick control that
does not fit a Pillow-shaped API.

**Stage 01 status:** this is the *core engine*. Loading, saving, metadata, the
fundamental operations and a low-level foundation are implemented and tested.
Exposing the complete ImageMagick feature set is a long-term goal and is
**not** complete here. See [Current limitations](#current-limitations).

---

## Table of contents

- [Why](#why)
- [Architecture](#architecture)
- [Requirements](#requirements)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Supported operations](#supported-operations)
- [Core operations](#core-operations)
- [Colour, channels and precision](#colour-channels-and-precision)
- [Image I/O and formats](#image-io-and-formats)
- [Pixel access](#pixel-access)
- [Metadata and the `mode` caveat](#metadata-and-the-mode-caveat)
- [Mutation semantics](#mutation-semantics)
- [High-level vs low-level API](#high-level-vs-low-level-api)
- [Error handling](#error-handling)
- [Threading](#threading)
- [Testing](#testing)
- [Benchmarks](#benchmarks)
- [Current limitations](#current-limitations)
- [Roadmap](#roadmap)
- [License](#license)

---

## Why

Pillow is excellent, but it only covers the formats and operations its own
libjpeg/libpng builds happen to include. ImageMagick supports hundreds of
formats and decades of accumulated filters. magik's aim is to give you both:

* a **simple API** for the 95% of work that needs no exotic knobs;
* a **complete, unhurried escape hatch** to everything ImageMagick can do, without
  the high-level API having to know about any of it.

magik is **inspired by Pillow, not a Pillow clone.** Where Pillow's semantics
are a poor fit for ImageMagick, magik does something explicit and documents the
difference rather than pretending otherwise. See
[`mode` caveat](#metadata-and-the-mode-caveat) and
[Crop](#supported-operations) for examples.

---

## Architecture

Four layers, strictly one-directional. Dependencies are never reversed.

```text
Python  ->  magik (python/magik)        public API, Pillow-shaped
              |
              v
         PyO3 bindings (src/)            argument coercion, panic containment,
              |                           Rust error -> Python exception
              v
         magik-core (crates/magik-core)  safe Rust image abstraction,
              |                           owns the MagickWand, no unsafe
              v
         magik-sys (crates/magik-sys)    raw FFI; ALL unsafe code lives here
              |
              v
         MagickWand / ImageMagick
```

| Crate | Responsibility | May depend on |
|---|---|---|
| `magik-sys` | `extern "C"` declarations, C types and enum constants, **plus the safe ownership wrappers** (`Wand`, `PixelColor`) that hold every raw pointer. **All `unsafe`.** | nothing |
| `magik-core` | Safe, idiomatic Rust: semantics, validation, error classification, pixel modes. **No `unsafe`.** | `magik-sys` |
| `_magik` (root) | PyO3 bindings. Knows Python; knows nothing about ImageMagick internals. | `magik-core` |
| `python/magik` | The API you import. | `_magik` |

Two invariants worth calling out, because they are what keep the design honest:

* **`magik-core` contains zero `unsafe`.** Not "no unsafe in its logic" — none at
  all. Every `unsafe` block in the workspace lives in
  `crates/magik-sys/src/wand.rs`, wrapping individual FFI calls. The files that
  hold the actual logic (`magik-core/src/image.rs`, `magik-core/src/pixel.rs` and
  the whole PyO3 layer) have none. Verify it yourself:

  ```console
  $ rg --pcre2 '^(?!\s*//)(?!\s*//!)(?!\s*\*).*\bunsafe\b.*$' crates/magik-core src
  # no output
  ```

* **`magik-core` never mentions Python.** It has no PyO3 dependency, so the Rust
  core is usable, and testable, as a plain Rust library (`cargo test -p
  magik-core` runs the full suite with no Python in the process).

### Why the ownership wrappers live in `magik-sys`

`magik-core` is only able to contain zero `unsafe` if something owns the raw
pointers and guarantees their lifetimes, and that has to be the crate that
declares the FFI. So `magik-sys` is not a bare list of declarations: it also
exposes `Wand` and `PixelColor`, which are *safe* Rust types that destroy their
handles on drop. `magik-core` holds a `Wand` behind a `Mutex` and only ever calls
safe methods on it. No `MagickWand*` is reachable outside `magik-sys`.

Two guard rails in that layer are worth knowing about, because they close real
hazards rather than theoretical ones:

* **Buffer lengths are validated before every pixel transfer.** ImageMagick's
  `MagickExportImagePixels` writes `columns * rows * channels * sizeof(storage)`
  bytes into the destination regardless of its length — an undersized buffer is
  overrun silently, not rejected. `magik-sys` checks the extent first.
* **Channel maps are validated.** `SetPixelChannelMap`, which interprets the
  `map` argument, is an ImageMagick *internal* function with no header
  declaration, and it silently degrades an unrecognised string to a CMYK map.
  magik restricts maps to the characters ImageMagick actually honours.

`magik-core` also bounds-checks every pixel coordinate, because
`MagickGetImagePixelColor` does **not**: an out-of-range coordinate silently
returns an unrelated pixel instead of failing.

### Keeping the backend swappable

The long-term goal is to be able to move to a different native engine without
rewriting the Python API. The layering is arranged so that:

* the Python surface is expressed in terms of *operations* (resize, crop, save),
  not ImageMagick concepts, so only the implementation of those operations would
  move;
* anything genuinely ImageMagick-specific lives behind `image.magick`, where it
  can be re-implemented or removed without disturbing the high-level API;
* `magik-sys` is a compile-time dependency of exactly one crate, so replacing the
  engine is a change inside `crates/` rather than across the tree.

---

## Requirements

* **Python** 3.11+
* **Rust** stable (developed against 1.99)
* **ImageMagick 7** with **MagickWand** development files (headers + import
  libraries)
* **maturin** >= 1.7

### ImageMagick specifics

magik links against ImageMagick **7.x**. It uses the MagickWand C API only, and
**never** invokes the `magick`, `convert` or `identify` executables, so it does
not depend on them being on `PATH`.

Which *formats* work depends on the delegates your ImageMagick was built with.
To see what yours supports:

```python
>>> import magik
>>> magick.version()
{'version': 'ImageMagick 7.1.2-32 Q16-HDRI x64 ...',
 'quantum_depth': 'Q16',
 'quantum_range': '65535'}
```

At install time, `magik-sys/build.rs` locates ImageMagick by searching, in order:

1. the `MAGICK_HOME` environment variable;
2. Scoop (`%USERPROFILE%\scoop\apps\imagemagick\current`), newest version first;
3. Chocolatey and `C:\Program Files\ImageMagick-*`.

**magik currently builds and is tested on Windows only.** `build.rs` resolves the
MagickWand import libraries by their Windows names (`CORE_RL_MagickWand_`).
ImageMagick's Linux packages name them differently (`libMagickWand-7.Q16HDRI.so`),
so a Unix build fails at link time even though `/usr` and Homebrew prefixes are
searched. Supporting Unix means teaching `build.rs` to drive `pkg-config`; that
has not been done, and CI reflects this by running on `windows-latest`.

ImageMagick's *headers* are not needed. `magik-sys` declares each FFI function by
hand and includes no header, so the only link-time requirement is the import
libraries above; the `cargo:include=` that `build.rs` emits is currently unused.

Set `MAGICK_HOME` explicitly if you have several installations:

```powershell
$env:MAGICK_HOME = 'C:\Program Files\ImageMagick-7.1.2-Q16-HDRI'
```

#### Loading the native libraries at run time

The extension module links `CORE_RL_MagickWand_.dll` **statically as an import
library**, so Windows must resolve that DLL *before* Python executes any magik
code. Either:

* add the ImageMagick directory to `PATH` (what the Scoop installer does), or
* keep the ImageMagick DLLs next to the installed `magik/_magik*.pyd`.

During `cargo build` / `cargo test`, `build.rs` copies the DLLs next to the
artifacts, so those work with no setup. For an installed package, make sure the
ImageMagick directory is on `PATH`.

---

## Installation

```bash
git clone <repository-url> magik
cd magik
python -m venv .venv
.\.venv\Scripts\activate          # Windows
# source .venv/bin/activate       # Linux / macOS

pip install maturin
maturin develop --release
```

`maturin develop` compiles the extension and installs it into the active virtual
environment. To build a distributable wheel instead:

```bash
maturin build --release
pip install target/wheels/magik-*.whl
```

Requires a virtual environment or conda environment (maturin's requirement).

### Toolchain

The Rust toolchain is pinned by `rust-toolchain.toml` to the stable channel with
the `rustfmt` and `clippy` components. Installing [rustup](https://rustup.rs) is
enough — any cargo invocation from the repository root fetches whatever the pin
names:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The pin exists so that a nightly rustup bump cannot turn a build red for reasons
unrelated to the code, and so a contributor's `cargo fmt` behaves exactly like
CI's.

### Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request against `main`:

* **`rust`** — formatting, `clippy -D warnings`, a release build, the workspace
  test suite, and a grep that fails if any `unsafe` appears outside `magik-sys`.
* **`python`** — Python 3.11 (the floor declared in `pyproject.toml`) and 3.14
  (current), each building the extension, running `pytest`, then building a wheel
  and re-running the suite against that wheel in a clean virtual environment.

Testing both ends of the range is deliberate: 3.9 stayed declared as the floor
here for a year after it reached end of life, because nothing in CI asserted
otherwise.

Both jobs first locate ImageMagick's **MagickWand import library**
(`lib\CORE_RL_MagickWand_.lib`), using whatever the runner image provides and
falling back to a pinned download of ImageMagick 7.1.2-32 when it is absent, then
exporting `MAGICK_HOME`.

When it has to download, CI **extracts** the installer with
[innoextract](https://github.com/dscharrer/innoextract) rather than running it.
That is not a preference: the import library lives in a non-default installer
component, so a silent install exits 0 and reports "Installation process
succeeded" while leaving no import libraries behind. Extraction takes the whole
payload. Scoop does the same thing, which is why a Scoop install has the
libraries and an installed one does not.

Headers are *not* required: `magik-sys` declares every FFI function by hand and
includes no ImageMagick header, so only the import libraries matter. The
`cargo:include=` that `build.rs` still emits is unused.

CI runs on **Windows** by necessity, not preference: those import libraries carry
their Windows names, whereas ImageMagick's Linux packages name them differently.
Supporting Linux would mean extending `build.rs` to use `pkg-config`; that is a
separate piece of work, not a runner swap.

---

## Quick start

```python
from magik import Image

image = Image.open("input.jpg")

print(image.width)     # 1920
print(image.height)    # 1080
print(image.format)    # "JPEG"
print(image.mode)      # "RGB"

image = image.resize(800, 600)
image = image.grayscale()

image.save("output.png")
```

Loading and saving without touching the filesystem:

```python
data = image.to_bytes("webp")     # -> bytes
same = Image.from_bytes(data)     # -> Image

from io import BytesIO
Image.open(BytesIO(data))         # file-like also works
```

The output format follows the filename extension unless you say otherwise, and
an explicit `format=` always wins over a misleading extension:

```python
image.save("output.jpg")                    # JPEG, from the extension
image.save("thumb.png")                     # PNG, from the extension
image.save("output.jpg", format="png")       # PNG, despite the .jpg name
```

Pillow-style argument forms are accepted too:

```python
image.resize((800, 600))        # same as image.resize(800, 600)
image.crop((10, 20, 210, 140))  # same as image.crop(10, 20, 210, 140)
```

---

## Supported operations

| Operation | Signature | Notes |
|---|---|---|
| Open from path | `Image.open(path)` | `str` or `os.PathLike` |
| Open from buffer | `Image.open(bytes)`, `Image.open(BytesIO(...))` | any file-like with `.read()` |
| Open (static) | `Image.from_bytes(data)` | any bytes-like (`bytes`, `bytearray`, `memoryview`) |
| Module shorthand | `magik.open(fp)` | identical to `Image.open` |
| **New image** | `Image.new(mode, size, color="black", depth=None)` | see [Image construction](#image-construction) |
| **Build from data** | `Image.from_pixels(mode, size, data, depth=None)` | packed samples, exact length |
| Save to path | `image.save(path, format=None)` | explicit `format` > extension > current |
| Save to buffer | `image.to_bytes(format=None)` | returns `bytes` |
| Save to stream | `image.save(file_object, format=None)` | anything with `.write()` |
| Copy | `image.copy()` | independent duplicate |
| Resize | `image.resize(w, h, filter=None)` | exact box, no aspect-ratio inference; default `lanczos` |
| Crop | `image.crop(l, t, r, b)` | `r`/`b` exclusive; **must be inside the image** |
| Rotate | `image.rotate(angle, background=None)` | **counter-clockwise**; `background` is keyword-only |
| Flip | `image.flip()` | vertical mirror (top ↔ bottom) |
| Flop | `image.flop()` | horizontal mirror (left ↔ right) |
| Grayscale | `image.grayscale()` | colorspace transform; **alpha preserved** |
| **Convert colorspace** | `image.convert_colorspace(name)` | genuine transform; see [conversion vs assignment](#converting-versus-assigning-a-colorspace) |
| **Extract a channel** | `image.extract_channel(name)` | single-channel result; names from `magik.channels()` |
| **Alpha presence** | `image.has_alpha` | read from ImageMagick, not inferred |
| **Convert depth** | `image.convert_depth(bits)` | 8 or 16; narrowing is irreversible |
| Blur | `image.blur(radius, sigma=None)` | Gaussian; `sigma` defaults to `radius / 2`; blurs alpha too |
| **Read one pixel** | `image.getpixel((x, y), depth=None)` | see [Pixel access](#pixel-access) |
| **Write one pixel** | `image.putpixel((x, y), value, depth=None)` | **returns a new image** |
| **Write a region** | `image.putpixels(origin, size, data, depth=None)` | **returns a new image** |
| **Read all pixels** | `image.pixels(depth=None)` | one ImageMagick call, one `bytes` |
| **Pixel mode** | `image.pixel_mode` | `"L"`, `"RGB"`, `"RGBA"`, ... |

`magik.filters()`, `magik.colorspaces()`, `magik.compressions()`,
`magik.channels()` and `magik.modes()` list the names accepted by `filter=`,
`with_colorspace()`, `with_compression()`, `extract_channel()` and the pixel API.

---

## Core operations

Every operation is **immutable**: it returns a new `Image` and leaves the receiver
untouched, so an image can safely be reused as the base for several results.

```python
thumb = image.resize(80, 60)
full  = image.resize(1920, 1080)   # `image` is still the original
```

### Resize

`image.resize(width, height, filter=None)` resamples to **exactly** that box. It
never infers an aspect ratio and never preserves one — asking for `20x5` gives a
20x5 image. Dimensions must both be greater than zero; `0` or a negative value
raises `MagikOperationError` rather than producing an empty image.

`filter` names the resampling kernel (`magik.filters()` lists them) and defaults
to `lanczos`. The same input and arguments always produce identical output.

### Crop

`image.crop(left, top, right, bottom)` uses Pillow's convention: **`right` and
`bottom` are exclusive**, so `crop(0, 0, 50, 40)` on a 100x80 image yields a
50x40 image.

The box **must lie inside the image**. Pillow silently pads out-of-bounds
requests with black; magik raises `MagikOperationError` instead, because padding
would require extra canvas work that magik does not do. A one-pixel box
(`crop(0, 0, 1, 1)`) is perfectly valid.

### Rotate — the size depends on the angle

This is the one operation whose output geometry is not obvious, so it is worth
stating plainly:

| Angle | Resulting size | Why |
|---|---|---|
| `0`, `180`, `360` | unchanged | exact multiples of a half turn |
| `90`, `270` | width and height **swapped** | exact quarter turns transpose |
| anything else | **larger in both axes** | the canvas grows so no corner is clipped |

So a 6x4 image rotated 90 degrees becomes 4x6, but rotated 45 degrees becomes
10x10. That growth is ImageMagick's `MagickRotateImage` expanding the canvas, and
magik passes it through unchanged. There is currently no way to request a fixed
canvas — Pillow's `expand=True`/`expand=False` split has no equivalent.

Positive angles rotate **counter-clockwise**, matching Pillow. ImageMagick's
native call turns the other way, so magik negates the angle internally.

`background` fills the corners exposed by an oblique rotation and is
**keyword-only**. It defaults to `"none"` (fully transparent), which means
`getpixel` at an exposed corner returns a 4-tuple even when the source was
3-channel:

```python
image.rotate(45).getpixel((0, 0))                       # (0, 0, 0, 0)
image.rotate(45, background="white").getpixel((0, 0))    # (255, 255, 255)
```

Non-finite angles (`nan`, `inf`) raise `MagikOperationError`.

### Grayscale — alpha is preserved

`image.grayscale()` converts colour to gray and **never drops or flattens
transparency**:

| Source | `mode` | `pixel_mode` | Samples per pixel |
|---|---|---|---|
| `RGB` | `L` | `L` | 1 |
| `RGBA` | `LA` | `RGBA` | **4** |

For an `RGBA` source the result reports `mode == "LA"` (Pillow's gray-plus-alpha
name) but returns **four** samples, `(gray, gray, gray, alpha)`, because magik's
pixel transfer has no 2-sample layout. The colour is replicated across the three
channels rather than stored once.

Applying it twice is a no-op, and an already-grayscale image is left alone.

### Blur

`image.blur(radius, sigma=None)` applies a Gaussian blur. `sigma` defaults to
`radius / 2`. `blur(0, 0)` returns an equivalent copy.

**Every channel is blurred, including alpha** — the same choice Pillow's
`GaussianBlur` makes. On an image with a hard transparency edge this softens the
alpha along with the colour, which is usually what you want but is not the same
as blurring colour alone.

Negative, `nan` or infinite parameters raise `MagikOperationError`.

### Flip and flop

`image.flip()` mirrors vertically (top ↔ bottom) and `image.flop()` mirrors
horizontally (left ↔ right). Neither changes the size, both are exact, and
applying either twice returns the original. They are distinct operations: on a
non-symmetric image the two produce different results, and they commute.

---

## Colour, channels and precision

### Converting versus assigning a colorspace

These are different operations that accept the same names, and they return
different numbers. For an `RGB` pixel of `(0, 0, 200)`:

```python
image.convert_colorspace("Gray").getpixel((0, 0))     # 14  <- recomputed luminance
image.magick.with_colorspace("Gray").getpixel((0, 0)) #  0  <- samples untouched
```

`convert_colorspace("Gray")` computes the luminance
(`0.2126·R + 0.7152·G + 0.0722·B` = 14.4). `with_colorspace()` only *declares*
that the existing sample values should be read as gray, so the first sample is
still the red channel — `0`. The latter lives on the low-level `image.magick`
handle; the high-level API exposes conversion, because silently misreading pixel
values is rarely what a caller wants.

Names come from `magik.colorspaces()` and are case-insensitive; `"Gray"`,
`"grey"` and `"l"` are accepted. An unknown name raises `MagikFormatError`.
Dimensions are preserved, and alpha survives when the destination can represent
it.

`image.grayscale()` is exactly `convert_colorspace("Gray")`, kept as a
convenience name.

### Channel extraction

```python
red   = image.extract_channel("red")     # single-channel "L" image
alpha = image.extract_channel("alpha")   # from an RGBA image
```

`magik.channels()` lists the accepted names: `red`, `green`, `blue`, `alpha`,
and the CMYK roles `cyan`, `magenta`, `yellow`, `black`. Single-letter aliases
(`"r"`, `"g"`, `"b"`, `"a"`, `"c"`, `"m"`, `"y"`, `"k"`) also work.

The result keeps the source's width and height and is single-channel, so it
behaves like any other image — readable with `getpixel()`/`pixels()`, savable,
chainable.

**Validation is not cosmetic.** ImageMagick's channel constants are *positional*
aliases — `red`, `cyan` and `gray` all mean "the first channel" — so asking for
the wrong one would otherwise return a plausible but meaningless image. magik
checks the request against the image's own type first:

```python
Image.new("L", (4, 4), 128).extract_channel("red")     # MagikOperationError
Image.new("RGB", (4, 4), (1, 2, 3)).extract_channel("alpha")  # MagikOperationError
```

Requesting `alpha` from an image without one is an **error, not an empty or
opaque image** — there is no defensible answer, so magik declines to invent one.

### `has_alpha`

```python
image.has_alpha    # -> bool
```

Read from ImageMagick's own alpha state, **not** inferred from `mode`,
`channels` or the filename. Those cannot be trusted:

| image | `channels` | `has_alpha` |
|---|---|---|
| `RGB` | 3 | `False` |
| `RGBA` | 4 | `True` |
| `CMYK` | 4 | **`False`** |
| grayscale+alpha PNG | 2 | `True` |

CMYK is the case that matters: four channels, no alpha. Any implementation
guessing from the channel count reports `True` there. The property is read-only
and touches no pixels.

### Depth: three different numbers

magik keeps these apart, and conflating them is the usual source of confusion:

| Name | What it is |
|---|---|
| `image.depth` | the image's own storage depth, as ImageMagick reports it |
| `depth=` on `pixels()`/`getpixel()`/`from_pixels()` | the **transfer** width: 8 or 16 |
| ImageMagick's `Quantum` | the backend's internal precision — not exposed |

`image.depth` can under-report. An image built with `depth=16` may still report
`8`, because it reflects the samples' current range rather than how they were
supplied.

### `convert_depth`

```python
lower = image.convert_depth(8)
higher = image.convert_depth(16)
```

Only 8 and 16 are accepted; anything else raises `MagikFormatError`.

**Widening and narrowing are not symmetric, and this is the important part:**

* **Widening (8 → 16) is free.** Eight-bit samples are exactly representable at
  sixteen, so `8 → 16 → 8` returns every original sample unchanged.
* **Narrowing (16 → 8) is irreversible.** It quantises, and the discarded low
  bits do not come back.

```python
fine = Image.from_pixels("L", (1, 1), b"\x00\x01", depth=16)  # value 1
fine.pixels(depth=16)                              # b'\x00\x01'
fine.convert_depth(8).convert_depth(16).pixels(depth=16)   # b'\x01\x01'  <- not b'\x00\x01'
```

Increasing the nominal precision therefore only ever *loses* detail deliberately;
it never recovers what a narrowing discarded.

### What is lossless, and what is not

| Operation | Lossless? |
|---|---|
| `has_alpha` | **Yes** — read-only, touches no pixels |
| `extract_channel` values | **Yes** — the selected channel is exact |
| `extract_channel` as a whole | **No** — a projection; the other channels are gone |
| `convert_colorspace` to the space already active | **Yes** — an exact no-op |
| `convert_depth` 8 → 16 | **Yes** — samples unchanged |
| `convert_depth` 16 → 8 | **No** — quantised irreversibly |
| `grayscale()` | **No** — collapses three channels to one |
| `convert_colorspace` between different spaces | **Usually, but see below** |

#### Colorspace round trips: measured, not promised

Round trips through an *invertible* colorspace were measured to be **bit-exact
at 8 bits** and **lossy at 16**, on ImageMagick 7.1.2-32 Q16-HDRI:

| Space | 8-bit round trip | 16-bit round trip |
|---|---|---|
| `sRGB` (identity) | exact | exact |
| `HSL` | exact | exact |
| `YCbCr` | exact | drift ≤ 23 / 65535 |
| `Lab`, `Luv`, `XYZ` | exact | drift ≤ 89 / 65535 |

Those figures are **observations of one ImageMagick build**, pinned by the test
suite so a change is noticed. They are not guarantees: a different build or
version may differ, and the 16-bit bounds in the tests are deliberately far
looser than what was measured so they survive such a change. Treat colorspace
conversion as potentially lossy and round in 8-bit if exactness matters.

Collapsing to `Gray` (or any other single channel) is lossy by construction and is
never reversible — that is a property of the operation, not a defect.

---

## Image I/O and formats

### The four entry points

| Call | Accepts | Returns |
|---|---|---|
| `Image.open(source)` | path (`str` / `os.PathLike` / `pathlib.Path`), any bytes-like, any object with `.read()` | `Image` |
| `Image.from_bytes(data)` | `bytes`, `bytearray`, `memoryview`, any buffer | `Image` |
| `image.save(target, format=None)` | path, or any object with `.write()` | `None` |
| `image.to_bytes(format=None)` | - | `bytes` |

`save_to_bytes()` is an alias of `to_bytes()`, and module-level `magik.open()`
matches `Image.open()`.

Paths are handed to ImageMagick to read directly; magik does not slurp the file
into Python first. Bytes-like sources go through ImageMagick's blob reader.

`to_bytes()` copies the encoded buffer out of ImageMagick before the wand is
released, so the returned `bytes` stays valid after the source image is dropped.

### Supported formats

Coverage is whatever the **installed ImageMagick build** provides, because
magik adds no codecs of its own. Verified here against ImageMagick
7.1.2-32 Q16-HDRI with png/jpeg/tiff/webp/gif/bmp delegates:

| Format | Read | Write | Notes |
|---|---|---|---|
| PNG | yes | yes | lossless; see the palette note below |
| JPEG | yes | yes | **lossy** - never compare exact samples |
| WebP | yes | yes | lossy by default |
| GIF | yes | yes | quantised to a 256-entry palette |
| TIFF | yes | yes | lossless; preserves the image type |
| BMP | yes | yes | reports a variant: `BMP`, `BMP2` or `BMP3` |
| MIFF | yes | yes | ImageMagick's native container; backs `Image.new` |

The test suite probes each format rather than assuming it, and skips when a
delegate is genuinely absent, so a differently-configured build stays green.
`magik.version()` reports the linked build's version, quantum depth and quantum
range - use it to confirm *which* ImageMagick is in play; whether a given coder is
present is a property of that build's delegates.

### `format` is not `mode`

These are different questions and magik keeps them apart:

```python
image.format   # "PNG"   -- the container/coder the image was decoded from
image.mode     # "RGB"   -- the pixel layout
```

An image can be a PNG and simultaneously `L`, `RGB`, `RGBA` or `P`.

### Choosing the output format

Resolution order for `image.save(path)`:

1. an explicit `format=` argument;
2. otherwise the filename extension;
3. otherwise the format the image was decoded from.

An explicit format is applied through ImageMagick's coder prefix, so a
misleading extension cannot override it:

```python
image.save("output.jpg", format="png")   # real PNG bytes in a .jpg file
```

**The format is validated before anything is written.** This matters more than it
looks: ImageMagick's writer *silently ignores a coder prefix it does not
recognise* and falls back to the extension, so `save("out.png", format="NOT-A-FORMAT")`
used to write a perfectly valid PNG and report success. It now raises
`MagikFormatError` and leaves no file behind. The same check applies to
`to_bytes()`.

### PNG re-encodes flat images - this is not a bug

ImageMagick rewrites repetitive images as palette (or 1-bit) PNGs, because for a
flat rectangle that is far smaller. A uniform `RGBA` image can therefore come back
as `mode == "P"` with three samples per pixel instead of four, and a
single-colour image can come back as `"L"`.

**The pixels you get back are still correct.** What changes is the storage
representation, which is ImageMagick's call, and therefore the number of samples
`pixels()` returns. Two practical consequences:

* do not assume `len(image.pixels())` is stable across a PNG round trip of a flat
  image;
* if you need the stored type preserved exactly, use **MIFF**, ImageMagick's own
  lossless container.

Real transparency is never dropped. magik reads a palette image that genuinely
carries transparency as RGBA - see
[Palette images read as resolved colour](#palette-images-read-as-resolved-colour).

### Lossy formats

JPEG and WebP do not reproduce the original samples, and magik does not pretend
otherwise. Round-trip tests for those formats assert the image is *visually*
intact (size preserved, differences within a per-channel tolerance) rather than
byte-identical. Exact-sample assertions are reserved for the lossless formats.

### Errors

```python
try:
    image.save("out.png", format="NOT-A-FORMAT")
except magik.MagikFormatError as exc:
    print(exc)                       # actionable, names the offending format
    print(exc.imagemagick_detail)     # ImageMagick's own reason, when it gave one
```

`MagickSetImageFormat` - the call that rejects an unknown coder - reports failure
**without recording a reason**, so magik supplies its own explanation rather than
passing on a bare C function name that tells you nothing. Where ImageMagick does
give a reason, it is preserved verbatim in `imagemagick_detail`.

A few ImageMagick coders (notably `NULL`) report success while writing nothing at
all. magik notices and raises `MagikSaveError` instead of letting the next
unrelated filesystem call fail with a confusing `FileNotFoundError`.

See [Error handling](#error-handling) for the full hierarchy.

### Runtime requirement

magik links against ImageMagick 7's MagickWand. **The ImageMagick DLLs must be
installed and discoverable at runtime** - they are not bundled into the wheel, so
`MAGICK_HOME` (or the loader's search path) has to resolve them. On Windows the
DLL directories are registered via `os.add_dll_directory` in
`magik/_native.py`, since `PATH` is not consulted for extension modules.

---

## Pixel access

```python
image = Image.open("input.png")

image.pixel_mode        # "RGB"
image.getpixel((0, 0))  # (12, 34, 56)
```

### Shape follows Pillow

`getpixel` returns an **`int`** for single-channel modes and a **`tuple`** for
everything else, matching Pillow:

| mode | samples | `getpixel` returns |
|---|---|---|
| `"1"`, `"L"` | 1 | `int`, e.g. `128` |
| `"P"` | 3 (resolved) | `tuple` |
| `"RGB"` | 3 | `tuple` |
| `"RGBA"` | 4 | `tuple` |
| `"CMYK"` | 4 | `tuple` |

`putpixel` accepts the matching shape and returns a **new** image:

```python
edited = image.putpixel((0, 0), (255, 0, 0))
edited.getpixel((0, 0))   # (255, 0, 0)
image.getpixel((0, 0))    # unchanged
```

Out-of-range sample values are clamped into range; an out-of-range coordinate
raises `MagikOperationError`.

### Bulk access is the fast path

`pixels()` returns the whole image as packed `bytes` in **one** ImageMagick call:

```python
raw = image.pixels()               # width * height * channels bytes
raw16 = image.pixels(depth=16)     # two bytes per sample, native endian
```

Samples are interleaved in row-major order. `putpixels` is the matching bulk
write:

```python
patched = image.putpixels((10, 10), (32, 32), region_bytes)
```

**Prefer these over loops.** Measured on this machine (see
[Benchmarks](#benchmarks)), one `getpixel` costs roughly **1 us**, while
`pixels()` works out at about **0.013 us per pixel** - around **80-100x cheaper
per pixel**, because one call replaces a Python round trip *plus* an FFI call per
pixel. `pixels()` also crosses the Python/Rust boundary exactly once.

### Sample depth

`depth=8` (the default) gives one byte per sample; `depth=16` gives two, in
native (little) endian order.

magik **never infers** this from `image.depth`, for two reasons that were both
measured rather than assumed: `Quantum` is `float` on HDRI builds and
`unsigned short` otherwise, so its width is not knowable at compile time; and
`MagickGetImageDepth` reports the *minimum* precision an image needs, so a
16-bit-capable wand can still report `8`. magik instead always asks ImageMagick
for an explicit integer sample type and lets it do the scaling.

### Palette images read as resolved colour

A `"P"` image is quantised by ImageMagick, so its pixels are stored as palette
*entries* rather than colour components. magik reads them as **resolved colour**
rather than as indices, because a one-channel read would hand back the red
component of the palette colour, which is misleading. Palette **indices** are not
exposed.

The sample count follows the transparency that is actually there:

| ImageMagick reports | `pixel_mode` | samples/pixel |
|---|---|---|
| `Palette` | `P` | 3 |
| `PaletteAlpha`, `PaletteBilevelAlpha` | `RGBA` | 4 |

ImageMagick only keeps an alpha channel when some pixel is genuinely not opaque.
A fully opaque image comes back as a plain `Palette`, so dropping to 3 samples
there loses nothing. Where transparency *is* real, magik transfers all four
samples - reading such an image as RGB would report a half-transparent pixel as
opaque and a fully transparent one as black.

### Image construction

```python
Image.new(mode, size, color="black", depth=None)
Image.from_pixels(mode, size, data, depth=None)
```

`magik.modes()` returns `["1", "L", "P", "RGB", "RGBA", "CMYK"]`.

New images are **MIFF-backed** - ImageMagick's native lossless format - so
`image.format` reports `"MIFF"` until the image is written, rather than claiming
to be a PNG it is not.

`from_pixels` requires **exactly** `width * height * channels * (depth // 8)`
bytes and rejects anything else with `MagikOperationError`.

### Colour parsing in `Image.new`

Numbers and sequences are **absolute sample values**, matching Pillow:

```python
Image.new("L", (8, 8), 128)                  # mid-grey
Image.new("RGB", (8, 8), (255, 0, 0))        # red
Image.new("RGBA", (8, 8), (255, 0, 0, 128))  # half-transparent red
```

They are not read as `0..1` fractions - `1` meaning "white" and `1` meaning
"almost black" is not a distinction worth making by guesswork. For anything
richer, pass a colour string and let ImageMagick parse it:

```python
Image.new("RGB", (8, 8), "#00ff00")
Image.new("RGB", (8, 8), "rgba(255,0,0,0.5)")
```

One sharp edge is documented because it bit us during development:
ImageMagick's `rgba()` reads its alpha as a **0..1 fraction**, not a 0..255
sample. `rgba(255,0,0,128)` comes out fully opaque, while
`rgba(255,0,0,0.501961)` really is half transparent. magik converts
automatically, so passing `(255, 0, 0, 128)` to `Image.new` does what you expect.

### Deliberate differences from Pillow (pixel API)

* **`putpixel` returns a new image.** Pillow mutates in place; magik's
  immutability rule applies here too. `putpixel` therefore costs a
  copy-on-write clone of the whole image, which is why a Python loop of
  `putpixel` calls is expensive - see the benchmark caveat below.
* **Palette indices are not exposed.** See above.
* **`mode` remains a projection.** See [`mode` caveat](#metadata-and-the-mode-caveat).

### Deliberate differences from Pillow

* **Crop does not pad.** Pillow silently expands the canvas when the box hangs
  over an edge. magik raises `MagikOperationError`, because padding needs extra
  canvas operations that Stage 01 does not implement.
* **Rotate does not expand.** `rotate(30)` keeps the original canvas; Pillow's
  `expand=True` has no equivalent yet. `background=` chooses the fill colour
  (transparent by default).
* **Rotate is counter-clockwise.** ImageMagick's native `MagickRotateImage` turns
  clockwise; magik negates the angle so the Python-facing direction matches
  Pillow's.
* **The filename extension is authoritative when saving.** Writing a JPEG to
  `thumb.png` produces a real PNG, because ImageMagick would otherwise write
  JPEG bytes into a file named `.png`. An explicit `format=` also wins over a
  misleading extension — magik applies it through ImageMagick's coder prefix
  (`PNG:out.jpg`) rather than setting the wand's format, because
  `MagickWriteImage` gives the extension precedence. When there is no
  recognisable extension and no explicit format, the image's own format is used.
* **The filename extension is also a hint when reading.** This is ImageMagick's
  behaviour, not magik's: opening a PNG that happens to be called `x.jpg` may be
  reported as JPEG. If you need format detection from the content alone, open the
  bytes rather than the path.
* **`save()` never mutates the image.** The output format is applied for the
  duration of the write only.

---

## Metadata and the `mode` caveat

Available directly on the image:

```python
image.width, image.height     # int
image.format                  # "PNG", "JPEG", "GIF", ...
image.mode                    # Pillow-style: "RGB", "RGBA", "L", "LA", "P", "1", "CMYK"
image.channels                # colour channels
image.depth                   # bits per channel (NOT bits per pixel)
image.image_type              # ImageMagick's own classification
image.colorspace              # "sRGB", "Gray", ...
image.compression             # "Zip", "JPEG", ...
image.compression_quality     # 0..100
image.size                    # (width, height)
image.filename                # source path, or None
image.metadata()              # all of the above as a dict
```

Three of these deserve an explanation, because ImageMagick and Pillow genuinely
disagree and magik does not paper over it.

**`mode` is a projection, not a native value.** ImageMagick has no `mode`
concept; it classifies images with `ImageType`. magik maps that onto Pillow's
vocabulary:

| ImageMagick `image_type` | magik `mode` |
|---|---|
| `Bilevel` | `1` |
| `Grayscale` | `L` |
| `GrayscaleAlpha` | `LA` |
| `Palette`, `PaletteAlpha`, `PaletteBilevelAlpha` | `P` |
| `TrueColor`, `Optimize` | `RGB` |
| `TrueColorAlpha` | `RGBA` |
| `ColorSeparation`, `ColorSeparationAlpha` | `CMYK` |

`PaletteAlpha` and `ColorSeparationAlpha` collapse to `P` and `CMYK` because
Pillow keeps an alpha/transparency channel *outside* the mode string, and magik
follows that convention so `mode` stays comparable. If you need to know about
transparency, check `image_type` or the alpha channel itself.

**`channels` is derived, not read.** ImageMagick 7.1's MagickWand exposes *no*
"how many channels does this image have" getter. magik derives the count from
the image's structural classification (`ImageType`), which is the same
information ImageMagick itself uses to decide how many samples a pixel holds.

**`depth` is per channel.** `depth == 16` on a `Q16` build means 16 bits per
channel, so an RGBA image is 64 bits per pixel. It is not Pillow's per-pixel
number.

---

## Mutation semantics

**Operations are immutable.** Every geometry and colour method returns a *new*
`Image` and leaves the receiver untouched.

```python
original = Image.open("input.jpg")
small   = original.resize(100, 100)

original.width   # unchanged: still the full size
small.width      # 100
```

This is a deliberate, project-wide rule — one rule, applied everywhere,
including the low-level API where `with_option()` and friends also return new
images rather than mutating.

Why:

* **No aliasing surprises.** An `Image` can never change underneath someone who
  is still holding it, which is the usual source of bugs in mutable image
  pipelines.
* **Errors stay visible.** An operation that fails raises immediately at the call
  site instead of recording a silent failure inside a wand.
* **Encoder settings stay isolated.** Each derived image owns its wand, so a JPEG
  quality tweak cannot leak into an unrelated PNG written later.

The cost is a wand clone per operation. ImageMagick's `CloneMagickWand` copies
the image *structure* and shares the pixel cache, so this is a structure copy, not
a full pixel-buffer copy. `benches/` measures the real cost; explicit in-place
variants can be added later if the numbers warrant it.

If you never need the original, overwriting the variable is the idiomatic form:

```python
image = image.resize(800, 600)
image = image.grayscale()
```

---

## High-level vs low-level API

The high-level API is lossy on purpose — it exposes seven metadata fields and
seven operations. `image.magick` is the escape hatch for everything else.

```python
low = image.magick

# raw ImageMagick information, not projected through Pillow's vocabulary
low.image_type          # "TrueColor"
low.colorspace          # "sRGB"
low.colorspace_value    # 23  (raw ColorspaceType)
low.compression         # "Zip"
low.compression_value   # 20  (raw CompressionType)
low.depth
low.channels
low.info()              # every field as a dict

# ImageMagick build information
low.version             # {'version': ..., 'quantum_depth': 'Q16', ...}
magik.version()         # same, without needing an image

# encoder settings that have no Pillow equivalent
low.get_option("png:bit-depth")                  # str or None
image2 = low.with_option("jpeg:sampling-factor", "4:4:4")
image3 = low.with_compression_quality(85)
image4 = low.with_compression("WebP")
image5 = low.with_colorspace("CMYK")
```

`image.magick` is a **live view** for reads and **functional** for writes,
matching the high-level API's immutability rule.

Stage 01 opens only this foundation. Hundreds of MagickWand functions are still
unreachable; each will be added here as a method, and none of them should require
changing the high-level API.

---

## Error handling

```text
MagikError                     (Exception)
├── MagikOpenError
├── MagikSaveError
├── MagikOperationError
├── MagikFormatError
│   └── MagikUnsupportedFormatError
└── MagikInternalError
```

Each `magik-core` error kind maps onto exactly one of these. An image format this
ImageMagick build has no delegate for is reported as `MagikUnsupportedFormatError`
(so it can be caught either as that or as `MagikFormatError`).

Every exception carries two attributes:

```python
try:
    magik.Image.open("missing.png")
except magik.MagikOpenError as exc:
    print(exc.kind)                 # "open"
    print(exc.imagemagick_detail)   # ImageMagick's own message, or None
```

The underlying ImageMagick message is appended to the Python message *and*
preserved on the attribute — errors are translated, never swallowed.

Three guarantees hold at the boundary:

* **No Rust panic reaches CPython.** Every public entry point runs inside a
  `catch_unwind` guard that converts a panic into `MagikInternalError`.
* **No raw `MagickWand` pointer is exposed.** Python only ever sees owned
  `Image` objects; the wand lives behind `magik-core`.
* **No subprocess.** ImageMagick is called in process, so a failure cannot be
  masked by a missing executable.

---

## Threading

An `Image` may be shared between Python threads.

This is not an unchecked promise. ImageMagick permits a wand to be *moved*
between threads but not *used* from two threads at once, so `magik-core` keeps
the wand behind a `Mutex`, which is exactly the required serialisation. As a
result `Image` is soundly `Send + Sync`, and copies produced by an operation or
`copy()` own their own wand and can be used in parallel.

```python
from concurrent.futures import ThreadPoolExecutor

image = magik.Image.open("input.jpg")
with ThreadPoolExecutor(max_workers=4) as pool:
    results = list(pool.map(lambda _: image.resize(64, 64).to_bytes("PNG"), range(8)))
```

Notes and limits:

* `Image` is `Send + Sync`; the raw `Wand` handle is deliberately **not** `Sync`,
  so it can never be shared by reference.
* magik does **not** release the GIL around wand calls, so Python-level
  parallelism is limited by the GIL for these operations. That is intentional:
  correctness and predictable behaviour first.
* There is **no async support** in Stage 01, by design.

---

## Testing

```bash
.venv\Scripts\activate
maturin develop --release
pytest -v
```

255 Python tests cover loading (path / bytes / stream / `PathLike`), metadata,
every operation, encoding, the exception hierarchy, the low-level namespace,
round trips, threading, and the absence of subprocesses and temp files. The Rust
core has its own 27-test suite plus doctests:

```bash
cargo test                       # Rust core, no Python involved
cargo clippy --workspace --all-targets
```

The suite generates its own fixtures at run time — PNG and BMP are assembled
*by hand* from `zlib`/`struct` so that decoding is tested against data ImageMagick
never produced, and a hand-written 1x1 GIF89a is included for the same reason.
JPEG, TIFF and WebP fixtures are transcoded from that hand-built PNG. Tests
**detect** which delegates the installed ImageMagick supports and skip — rather
than fail — when a format is genuinely unavailable.

---

## Benchmarks

```bash
python benches/benchmark.py --iterations 20          # everything
python benches/benchmark.py --pixels --iterations 5   # pixel scenarios only
python benches/benchmark.py --io --iterations 3       # image I/O scenarios only
python benches/benchmark.py --only getpixel --json benches/results/pixels.json
```

These benchmarks exist to establish a **baseline**, not to prove performance.
No optimisation has been attempted, deliberately.

**Stage 01 scenarios** cover loading (from path and from bytes), `resize`,
`crop`, `grayscale`, `blur`, `flip`/`flop`, `rotate`, `copy`, `metadata()`, and
saving (to bytes and to a path, PNG and JPEG), over generated inputs at 256x256,
1024x768 and 2560x1600.

**Stage 02 pixel scenarios** cover single-pixel `getpixel` and `putpixel`, bulk
`pixels()` extraction at 8 and 16 bits, `Image.new` and `Image.from_pixels`, at
**512x512, 1920x1080 and 3840x2160**.

**Stage 03 I/O scenarios** cover load and save for PNG, JPEG and WebP at
**640x480, 1280x720 and 1920x1080**, each measured twice - once against a path
and once against an in-memory buffer holding the same bytes.

Every scenario is warmed up, timed over a batch of iterations, and reported as
the best of several samples with a standard deviation — whole Python-facing calls
are timed, so the numbers include argument marshalling and are directly
comparable to what a caller experiences.

The run ends with a derived comparison of single-pixel versus bulk access. Two
results are worth calling out, and one caveat:

* **Reads are the reason the bulk API exists.** One `getpixel` costs about
  **1 us**, while `pixels()` works out at about **0.013 us per pixel** — roughly
  **80-100x cheaper per pixel** across all three sizes, because a single call
  replaces a Python round trip plus an FFI call per pixel.
* **Stage 01's `copy` result is what justifies immutability**: an operation's
  copy-on-write clone costs microseconds against millisecond-scale pixel work.
* **Caveat: `putpixel` and `from_pixels` are *not* comparable per pixel**, and
  the benchmark deliberately prints no ratio for them. `putpixel`'s cost is
  dominated by the copy-on-write clone of the whole image, not by writing one
  pixel, so a ratio would compare clone-against-bulk-import and say nothing about
  pixel cost. If you need to write many individual pixels, use `putpixels`.

### I/O is codec-bound, not disk-bound

The Stage 03 I/O scenarios split each measurement by differencing the path route
against the buffer route, which differ only in whether the filesystem is touched.
Measured on this machine (best of 1, 3 iterations — indicative only, not a
benchmark of record):

| Case | Load | Save |
|---|---|---|
| PNG 640x480 | 5.73 ms total = 5.23 ms decode + 0.51 ms fs | 20.05 ms = 18.99 ms encode + 1.05 ms fs |
| JPEG 640x480 | 7.22 ms total = 6.04 ms decode + 1.18 ms fs | 17.01 ms = 14.15 ms encode + 2.86 ms fs |
| WebP 640x480 | 8.27 ms total = 7.63 ms decode + 0.64 ms fs | 38.54 ms = 35.18 ms encode + 3.36 ms fs |

At 1920x1080 the same pattern holds: PNG load is 44.37 ms of which ~4 ms is the
filesystem, and PNG save is 125.45 ms of which ~7 ms is the filesystem.

The honest reading is that **almost all of the time is ImageMagick compressing
and decompressing**, with the filesystem a low-single-digit percentage. These are
raw measurements of one machine and one build; no speed-up against any other
library is claimed, because none was measured.

Results are also writable as JSON so a later stage can diff two runs:

```json
{
  "environment": { "magik": "0.1.0", "imagemagick": "ImageMagick 7.1.2-32 ...", "quantum_depth": "Q16" },
  "results": [ { "name": "resize.small_from_medium", "unit": "ms", "best": 18.39, "median": 18.49 } ]
}
```

---

## Current limitations

Known and intentional at this stage:

* **No iterators or flat sequences.** There is no `load()`, `getdata()`,
  `getcolors()` or `tobytes()` that hands back a list of per-pixel tuples. magik
  exposes packed buffers via `pixels()` and construction via `from_pixels()`,
  which is both faster and a smaller surface — but it is not Pillow's shape, and
  converting between them is left to the caller.
* **Palette indices are not exposed.** A `"P"` image reads back as resolved
  colour (3 samples, or 4 where real transparency is present); the palette table
  itself is not reachable through the pixel API.
* **PNG encoding re-classifies an image.** ImageMagick's PNG encoder optimises,
  so a uniform or repetitive image can come back as `"1"`, `"L"` or `"P"` rather
  than its original mode. This is ImageMagick behaviour, not a magik defect, and
  the pixels are still correct — but the **number of samples per pixel can
  change**, so `len(image.pixels())` is not stable across a PNG round trip of a
  flat image. Real transparency is never dropped. Use `MIFF` when you need an
  exact, unoptimised round trip.
* **No `quality=` knob.** `to_bytes()` and `save()` take only a format name. JPEG
  and WebP quality, subsampling and interlace settings are reachable through the
  low-level `image.magick` option API (`with_compression_quality`,
  `with_compression`), but there is no Pillow-style `quality=` keyword yet.
* **`rotate()` cannot use a fixed canvas.** Pillow's `expand=True`/`expand=False`
  has no equivalent: an oblique rotation always grows the image, because
  ImageMagick expands the canvas to avoid clipping. Requesting the original
  dimensions afterwards via `resize()` is the workaround.
* **Crop does not pad.** An out-of-bounds crop box raises rather than padding
  with black the way Pillow does.
* **`grayscale()` of `RGBA` returns four samples**, not the two that `mode == "LA"`
  implies, because the pixel transfer has no 2-sample gray+alpha layout.
* **No ICC profile management.** `with_colorspace()` and
  `convert_colorspace()` move between colorspaces using ImageMagick's built-in
  transforms; neither embeds, applies or manages ICC profiles, and there is no
  color-managed rendering pipeline. Colorspaces are interpreted as they are
  named, not via a profile.
* **Colorspace round trips are not guaranteed.** They were measured bit-exact at
  8 bits and lossy at 16 on one ImageMagick build. See
  [what is lossless](#what-is-lossless-and-what-is-not).
* **ImageMagick DLLs are not bundled.** The wheel links against MagickWand but
  does not ship it, so ImageMagick 7 must be installed and `MAGICK_HOME` (or the
  loader search path) must resolve it at runtime. On Windows the DLL directories
  are registered via `os.add_dll_directory` in `magik/_native.py`.
* **Sample depth is always explicit.** `depth=8` or `depth=16`; magik does not
  expose ImageMagick's native `Quantum` width, and does not infer it from
  `image.depth`. See [Sample depth](#sample-depth).
* **No drawing**, compositing, pasting, or text rendering.
* **No multi-frame handling.** No `seek`, animation, or frame iteration; magik
  operates on the first frame only.
* **No color management** beyond ImageMagick's own colorspace transforms (no ICC
  profiles, no embedded-profiles API).
* **Crop and rotate do not expand the canvas** and crop does not pad.
* **No in-place operations** — see [Mutation semantics](#mutation-semantics). In
  particular `putpixel` clones the image, so pixel-by-pixel edits in a Python
  loop are expensive by design; use `putpixels`.
* **No async**, no streaming/pipeline API, no plugin system.
* **The low-level API is a foundation only.** A small, deliberate set of
  MagickWand capabilities; not "the full ImageMagick API".
* **No wheel-level bundling of ImageMagick itself** — the native library is a
  system dependency that must be resolvable at run time.

**Full ImageMagick feature coverage is a long-term goal of this project and is
not complete.** magik deliberately ships a small, well-tested core rather than a
broad, shallow surface.

---

## Roadmap

Roughly, in dependency order:

1. ~~core image engine~~ (**Stage 01**)
2. ~~pixel access and buffer round-tripping~~ (**Stage 02**)
3. ~~image I/O and format handling~~ (**Stage 03**)
4. ~~core image operations: resize, crop, rotate, grayscale, blur, flip/flop~~ (**Stage 04**)
5. ~~colour spaces, channel extraction, alpha introspection, precision~~ (**Stage 05**)
6. compositing, pasting and drawing primitives
7. multi-frame / animated format support
8. broadening `image.magick` towards the MagickWand surface
9. keeping `magik-core` backend-agnostic enough for a second engine

---

## License

MIT — see [LICENSE](LICENSE).

magik links against ImageMagick, which is separately licensed
(<https://imagemagick.org/license.html>).
