use std::path::{Path, PathBuf};

fn main() {
    // Embed build provenance for on-device log verification.
    //
    // This avoids guessing what was last flashed when iterating quickly on hardware bring-up.
    // Keep it dependency-free (no vergen) and best-effort (do not fail builds if git isn't available).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=FW_BUILD_PROFILE={}", profile);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    // Re-run when firmware sources change so provenance always matches the artifact.
    emit_rerun_if_exists(&manifest_dir.join("build.rs"));
    emit_rerun_if_exists(&manifest_dir.join("Cargo.toml"));
    emit_rerun_for_dir(&manifest_dir.join("src"));

    // Ensure the build script re-runs when the git revision changes, even if firmware sources
    // didn't (e.g. commits touching only docs, or branch switches). Without this, Cargo can
    // keep an old `FW_GIT_SHA` and defeat provenance logging.
    //
    // Use `git rev-parse --git-dir` to support worktrees and non-root working directories.
    {
        let git_dir = std::process::Command::new("git")
            .current_dir(&manifest_dir)
            .args(["rev-parse", "--git-dir"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .map(|p| {
                if p.is_absolute() {
                    p
                } else {
                    manifest_dir.join(p)
                }
            });

        if let Some(git_dir) = git_dir {
            for rel in ["HEAD", "logs/HEAD", "packed-refs", "index"] {
                let path = git_dir.join(rel);
                emit_rerun_if_exists(&path);
            }
        }
    }

    let git_sha = {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&manifest_dir);
        cmd.args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    };
    println!("cargo:rustc-env=FW_GIT_SHA={}", git_sha);
    let src_hash = source_hash(&manifest_dir);
    let src_hash_hex = format!("{:016x}", src_hash);
    println!("cargo:rustc-env=FW_SRC_HASH={}", src_hash_hex);
    let git_dirty = git_dirty_state(&manifest_dir);
    println!("cargo:rustc-env=FW_GIT_DIRTY={}", git_dirty);
    println!(
        "cargo:rustc-env=FW_BUILD_ID={}-{}-{}",
        git_sha, git_dirty, src_hash_hex
    );

    // Only apply embedded linker scripts when building for Xtensa.
    //
    // This keeps `cargo test --lib` usable on the host while still producing
    // the correct link args for ESP32-S3 binaries.
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "xtensa" {
        // ESP32-S3 (Xtensa) app linking uses the esp-hal-provided linker scripts.
        println!("cargo:rustc-link-arg-bins=-Tlinkall.x");

        // Required by defmt (see https://defmt.ferrous-systems.com/setup#linker-script).
        println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    }
}

fn emit_rerun_if_exists(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn emit_rerun_for_dir(dir: &Path) {
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    files.sort();
    for path in files {
        emit_rerun_if_exists(&path);
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn source_hash(manifest_dir: &Path) -> u64 {
    let mut files = Vec::new();
    let cargo_toml = manifest_dir.join("Cargo.toml");
    let build_rs = manifest_dir.join("build.rs");
    if cargo_toml.is_file() {
        files.push(cargo_toml);
    }
    if build_rs.is_file() {
        files.push(build_rs);
    }
    collect_files(&manifest_dir.join("src"), &mut files);
    files.sort();

    let mut hash = 0xcbf29ce484222325u64;
    for path in files {
        let rel = path.strip_prefix(manifest_dir).unwrap_or(&path);
        hash_bytes(&mut hash, rel.to_string_lossy().as_bytes());
        hash_bytes(&mut hash, &[0]);
        if let Ok(content) = std::fs::read(&path) {
            hash_bytes(&mut hash, &content);
        }
        hash_bytes(&mut hash, &[0xff]);
    }
    hash
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn git_dirty_state(manifest_dir: &Path) -> &'static str {
    let output = std::process::Command::new("git")
        .current_dir(manifest_dir)
        .args([
            "status",
            "--porcelain",
            "--untracked-files=no",
            "--",
            "src",
            "Cargo.toml",
            "build.rs",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            if String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                "clean"
            } else {
                "dirty"
            }
        }
        _ => "unknown",
    }
}
