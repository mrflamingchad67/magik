"""The magik exception hierarchy.

.. code-block:: text

    MagikError                     (Exception)
    |-- MagikOpenError
    |-- MagikSaveError
    |-- MagikOperationError
    |-- MagikFormatError
    |   `-- MagikUnsupportedFormatError
    `-- MagikInternalError

These classes are defined here in Python rather than in the extension module so
that they have a real ``__module__``, a normal ``repr``, and a single obvious
home in the documentation.

The Rust core raises them by looking each class up in this module at import time
and instantiating it with the rendered message. It then attaches two diagnostic
attributes to the instance:

``kind``
    A short, stable string identifying the failure category (``"open"``,
    ``"save"``, ...). Handy for programmatic handling and for tests.

``imagemagick_detail``
    ImageMagick's own error message, verbatim, when the C layer produced one --
    otherwise ``None``. magik translates errors, it never swallows them.
"""

from __future__ import annotations

__all__ = [
    "MagikError",
    "MagikOpenError",
    "MagikSaveError",
    "MagikOperationError",
    "MagikFormatError",
    "MagikUnsupportedFormatError",
    "MagikInternalError",
]


class MagikError(Exception):
    """Base class for every error raised by magik."""

    #: Stable machine-readable failure category.
    kind: str = "error"
    #: The underlying ImageMagick message, when there was one.
    imagemagick_detail: str | None = None


class MagikOpenError(MagikError):
    """Raised when an image cannot be opened.

    Typical causes: the path does not exist, the file cannot be read, or the
    bytes are not a recognised image format.
    """

    kind = "open"


class MagikSaveError(MagikError):
    """Raised when an image cannot be written to a path or a buffer."""

    kind = "save"


class MagikOperationError(MagikError):
    """Raised when a geometry or colour operation fails.

    Typical causes: zero or negative dimensions, a crop box that is inverted or
    outside the image, or an unusable blur/rotation parameter.
    """

    kind = "operation"


class MagikFormatError(MagikError):
    """Raised when a format or codec name is unknown or unusable."""

    kind = "format"


class MagikUnsupportedFormatError(MagikFormatError):
    """Raised when the linked ImageMagick build has no delegate for a format.

    This is a :class:`MagikFormatError` subclass, so callers may catch either
    this specific case or the broader category.
    """

    kind = "unsupported_format"


class MagikInternalError(MagikError):
    """Raised when an ImageMagick internal invariant is violated.

    Reaching this means a bug in magik rather than a problem with the caller's
    input. It is also what a Rust panic is converted into, so that no panic can
    ever escape into Python. Please report it.
    """

    kind = "internal"