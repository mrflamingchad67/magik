//! Locates the native ImageMagick/MagickWand installation and emits the
//! necessary link directives.
//!
//! Responsibilities:
//!   * discover an ImageMagick prefix (`MAGICK_HOME`, Scoop, Chocolatey,
//!     `C:\Program Files\ImageMagick-*`, Homebrew, common Linux prefixes);
//!   * verify the MagickWand import library is present;
//!   * emit `cargo:rustc-link-search` / `cargo:rustc-link-lib` directives;
//!   * work with both MSVC-style (`CORE_RL_MagickWand_.lib`) and MinGW-style
//!     (`libCORE_RL_MagickWand_.dll.a`) import library layouts;
//!   * stage the ImageMagick runtime DLLs next to the build artifacts so
//!     `cargo build` / `cargo test` work without extra PATH setup.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Import libraries that must be linked.
const REQUIRED_LIBS: &[&str] = &["CORE_RL_MagickWand_", "CORE_RL_MagickCore_"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=MAGICK_HOME");

    let home = match discover_magick_home() {
        Some(dir) => dir,
        None => fail(
            "Could not locate an ImageMagick installation.\n\
                 Set the MAGICK_HOME environment variable to your ImageMagick prefix \
                 (the directory that contains the `lib` and `include` folders), \
                 e.g. `C:\\Program Files\\ImageMagick-7.1.2-Q16`.",
        ),
    };

    let lib_dir = home.join("lib");
    let include_dir = home.join("include");

    // Record the prefix so the crate can report it at runtime and, if needed,
    // add it to the DLL search path for delay-loaded dependencies.
    println!("cargo:rustc-env=MAGIK_MAGICK_HOME={}", home.display());
    println!("cargo:include={}", include_dir.display());

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    let mut linked: Vec<String> = Vec::new();

    for lib in REQUIRED_LIBS {
        // GNU/MinGW ld resolves `-lNAME` to `libNAME.dll.a` / `libNAME.a`, so an
        // MSVC `.lib` must be restaged under the name the linker will look for.
        let msvc = lib_dir.join(format!("{lib}.lib"));
        let gnu = lib_dir.join(format!("lib{lib}.dll.a"));
        let gnu_static = lib_dir.join(format!("lib{lib}.a"));

        if gnu.exists() || gnu_static.exists() {
            search_dirs.push(lib_dir.clone());
        } else if msvc.exists() {
            let staging = out_dir().join("implib");
            fs::create_dir_all(&staging).unwrap_or_else(|e| {
                fail(&format!("failed to create {}: {e}", staging.display()));
            });
            let dest = staging.join(format!("lib{lib}.dll.a"));
            fs::copy(&msvc, &dest).unwrap_or_else(|e| {
                fail(&format!(
                    "failed to stage import library {} -> {}: {e}",
                    msvc.display(),
                    dest.display()
                ));
            });
            search_dirs.push(staging.clone());
        } else {
            fail(&format!(
                "no MagickWand import library for `{lib}` in {}.\n\
                 Expected one of: {}.lib, lib{}.dll.a, lib{}.a",
                lib_dir.display(),
                lib,
                lib,
                lib
            ));
        }

        linked.push((*lib).to_string());
    }

    search_dirs.sort();
    search_dirs.dedup();
    for dir in &search_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    for lib in &linked {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }

    stage_runtime_dlls(&home);
}

/// The `OUT_DIR` cargo assigned to this build script.
fn out_dir() -> PathBuf {
    PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"))
}

/// Copies the ImageMagick runtime libraries next to the produced binaries.
///
/// The extension module statically imports `CORE_RL_MagickWand_.dll`, which
/// Windows must resolve *before* `main()` runs. Copying the DLLs next to the
/// artifact makes `cargo build` and `cargo test` self-contained.
fn stage_runtime_dlls(home: &Path) {
    let Ok(entries) = fs::read_dir(home) else {
        return; // Non-Windows or header-only install: PATH/pkg-config rules apply.
    };

    // OUT_DIR is `<target>/<profile>/build/<pkg>-<hash>/out`.
    let profile_dir = match out_dir().ancestors().nth(3) {
        Some(dir) => dir.to_path_buf(),
        None => return,
    };

    let mut dlls: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        {
            dlls.push(path);
        }
    }

    for dest_dir in [profile_dir.clone(), profile_dir.join("deps")] {
        if !dest_dir.is_dir() {
            continue;
        }
        for dll in &dlls {
            if let Some(name) = dll.file_name() {
                let _ = fs::copy(dll, dest_dir.join(name));
            }
        }
    }
}

/// Search order: explicit override, then well-known per-platform prefixes.
fn discover_magick_home() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(explicit) = env::var("MAGICK_HOME") {
        let trimmed = PathBuf::from(explicit.trim());
        if trimmed.as_os_str().is_empty() {
            println!("cargo:warning=MAGICK_HOME is set but empty; ignoring it.");
        } else {
            candidates.push(trimmed);
        }
    }

    if let Ok(user_profile) = env::var("USERPROFILE") {
        // Scoop keeps the active version behind a `current` junction.
        let scoop_root = format!("{user_profile}\\scoop\\apps\\imagemagick");
        candidates.push(PathBuf::from(&scoop_root).join("current"));
        if let Ok(entries) = fs::read_dir(&scoop_root) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "current"))
                .collect();
            // Highest version last => try the newest first after reversing.
            versions.sort_by_key(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.split('.').next().and_then(|m| m.parse::<u32>().ok()))
                    .unwrap_or(0)
            });
            versions.reverse();
            candidates.extend(versions);
        }
    }

    // Chocolatey and the classic Windows installer layout.
    candidates.push(PathBuf::from(
        "C:\\ProgramData\\chocolatey\\lib\\imagemagick\\tools",
    ));
    if let Ok(entries) = fs::read_dir("C:\\Program Files") {
        let mut installed: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("ImageMagick"))
            })
            .collect();
        installed.sort();
        installed.reverse();
        candidates.extend(installed);
    }

    // Unix-like prefixes (Homebrew, distro packages, manual installs).
    candidates.push(PathBuf::from("/opt/homebrew/opt/imagemagick"));
    candidates.push(PathBuf::from("/usr/local/opt/imagemagick"));
    candidates.push(PathBuf::from("/usr/local"));
    candidates.push(PathBuf::from("/usr"));

    candidates.into_iter().find(|dir| dir.join("lib").is_dir())
}

fn fail(message: &str) -> ! {
    // `cargo:warning` is shown even when a later error aborts the build, so the
    // user always sees the actionable message.
    for line in message.lines() {
        println!("cargo:warning={line}");
    }
    panic!("{message}");
}
