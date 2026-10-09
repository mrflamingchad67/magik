"""Native-library bootstrap for magik on Windows.

Why this exists
---------------

The magik extension module statically imports ``CORE_RL_MagickWand_.dll``.
Windows has to resolve that DLL **before** any magik code runs, and since
CPython 3.8 extension modules are loaded with ``LOAD_LIBRARY_SEARCH_DEFAULT_DIRS``
— a flag set that deliberately **excludes ``PATH``**. Adding ImageMagick to
``PATH`` is therefore not sufficient.

Two mechanisms remain: the DLLs sitting next to the ``.pyd``, or a directory
registered with :func:`os.add_dll_directory` before the import. This module
handles the second, using the same discovery order as ``magik-sys/build.rs`` so
a build and a run agree on which ImageMagick is being used.

Non-Windows platforms link differently (and consult the usual loader paths), so
this is a no-op there.
"""

from __future__ import annotations

import glob
import os

#: Handles returned by :func:`os.add_dll_directory`.
#:
#: They **must** be kept alive for as long as the process may load the DLLs:
#: CPython closes the directory as soon as the handle is garbage-collected,
#: which would silently break a later, unrelated import.
_handles: list[object] = []

#: Set when no usable ImageMagick directory was found, so ``__init__`` can
#: explain the failure instead of leaving a bare "DLL load failed".
_missing_reason: str | None = None


def candidate_directories() -> list[str]:
    """ImageMagick installation directories to try, most specific first.

    Mirrors the search in ``crates/magik-sys/build.rs`` so the runtime picks the
    same installation that the binary was linked against.
    """
    candidates: list[str] = []

    explicit = os.environ.get("MAGICK_HOME", "").strip()
    if explicit:
        candidates.append(explicit)

    user_profile = os.environ.get("USERPROFILE")
    if user_profile:
        scoop_root = os.path.join(user_profile, "scoop", "apps", "imagemagick")
        candidates.append(os.path.join(scoop_root, "current"))
        # Scoop also keeps versioned directories; newest first.
        versions = sorted(glob.glob(os.path.join(scoop_root, "[0-9]*")), reverse=True)
        candidates.extend(versions)

    candidates.append(r"C:\ProgramData\chocolatey\lib\imagemagick\tools")

    program_files = os.environ.get("ProgramFiles")
    if program_files:
        installed = sorted(glob.glob(os.path.join(program_files, "ImageMagick-*")))
        candidates.extend(reversed(installed))

    return candidates


def prepare() -> str | None:
    """Register ImageMagick's DLL directory with the OS loader.

    Returns ``None`` on success (and on non-Windows platforms, where nothing is
    needed), or a human-readable reason when no installation could be found.
    """
    global _missing_reason

    if os.name != "nt":
        return None

    if _handles:
        return None  # already prepared

    tried: list[str] = []
    for directory in candidate_directories():
        if not directory or not os.path.isdir(directory):
            continue
        tried.append(directory)
        try:
            _handles.append(os.add_dll_directory(directory))
        except OSError as error:  # pragma: no cover - depends on permissions
            _missing_reason = f"could not use {directory!r}: {error}"
            continue
        return None

    _missing_reason = (
        "no ImageMagick installation found. Set the MAGICK_HOME environment "
        "variable to your ImageMagick prefix (the directory containing the "
        "ImageMagick DLLs, e.g. 'C:\\\\Program Files\\\\ImageMagick-7.1.2-Q16'). "
        "Searched: " + (", ".join(tried) if tried else "<no candidates>")
    )
    return _missing_reason


def missing_reason() -> str | None:
    """The failure reason recorded by :func:`prepare`, if any."""
    return _missing_reason