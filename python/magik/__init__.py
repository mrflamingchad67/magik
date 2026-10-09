"""magik — a Pillow-inspired Python image API backed by ImageMagick.

magik is built on three layers:

* a **Python API** you use directly (:class:`Image` and friends);
* a **Rust core** (``magik-core``) that owns ImageMagick resources safely;
* a **raw FFI layer** (``magik-sys``) that is the only place ``unsafe`` exists.

All image work happens *in process* through the MagickWand C API. magik never
shells out to the ``magick``, ``convert`` or ``identify`` executables, and it
never materialises a temporary file to do an in-memory conversion.

Quick start
-----------

.. code-block:: python

    from magik import Image

    image = Image.open("input.jpg")
    print(image.width, image.height, image.format)

    image = image.resize(800, 600)
    image = image.grayscale()
    image.save("output.png")

Operations are **immutable**: each one returns a new :class:`Image` and leaves
the receiver untouched.

Low-level access
----------------

.. code-block:: python

    image.magick.image_type          # ImageMagick's own classification
    image.magick.version             # linked ImageMagick build information
    image = image.magick.with_option("jpeg:sampling-factor", "4:4:4")

Threading
---------

An :class:`Image` is safe to share between Python threads: the Rust core
serialises all access to the underlying MagickWand. There is no async support in
this release.
"""

from . import _native

# Windows must resolve ImageMagick's native library before the extension module
# can be loaded, and the default loader search does not consult PATH. This has to
# run before the `._magik` import below.
_native_failure = _native.prepare()

# The exception classes must exist before the extension module is initialised,
# because it resolves them during its own import.
from .exceptions import (  # noqa: E402
    MagikError,
    MagikFormatError,
    MagikInternalError,
    MagikOpenError,
    MagikOperationError,
    MagikSaveError,
    MagikUnsupportedFormatError,
)

try:
    from ._magik import (  # noqa: E402
        Image,
        MagickHandle,
        channels,
        colorspaces,
        compressions,
        default_filter,
        filters,
        modes,
        open,
        version,
    )
except ImportError as _exc:  # pragma: no cover - depends on the host install
    if _native_failure is not None:
        raise ImportError(
            f"magik could not initialise the ImageMagick native library: {_native_failure}"
        ) from _exc
    raise ImportError(
        "magik could not load the ImageMagick native library "
        "(CORE_RL_MagickWand_.dll). Make sure ImageMagick 7 is installed and its "
        "directory is registered with the loader; see the README section "
        "'ImageMagick specifics'."
    ) from _exc

__version__ = "0.1.0"

__all__ = [
    "Image",
    "MagickHandle",
    # exception hierarchy
    "MagikError",
    "MagikOpenError",
    "MagikSaveError",
    "MagikOperationError",
    "MagikFormatError",
    "MagikUnsupportedFormatError",
    "MagikInternalError",
    # module-level helpers
    "open",
    "version",
    "default_filter",
    "channels",
    "colorspaces",
    "compressions",
    "filters",
    "modes",
    "__version__",
]