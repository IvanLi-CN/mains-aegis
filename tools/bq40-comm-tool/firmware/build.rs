fn main() {
    // Embed build provenance for on-device log verification.
    //
    // This avoids guessing what was last flashed when iterating quickly on hardware bring-up.
    // Keep it dependency-free (no vergen) and best-effort (do not fail builds if git isn't available).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=FW_BUILD_PROFILE={}", profile);

    // Ensure the build script re-runs when the git revision changes, even if firmware sources
    // didn't (e.g. commits touching only docs, or branch switches). Without this, Cargo can
    // keep an old `FW_GIT_SHA` and defeat provenance logging.
    //
    // Use `git rev-parse --git-dir` to support worktrees and non-root working directories.
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
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
                    std::path::Path::new(&manifest_dir).join(p)
                }
            });

        if let Some(git_dir) = git_dir {
            // In worktrees, refs live in the common git dir (pointed to by `commondir`).
            let common_git_dir = git_dir
                .join("commondir")
                .exists()
                .then(|| std::fs::read_to_string(git_dir.join("commondir")).ok())
                .flatten()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .map(|p| if p.is_absolute() { p } else { git_dir.join(p) })
                .unwrap_or_else(|| git_dir.clone());

            // Worktree-scoped files.
            for rel in ["HEAD", "logs/HEAD", "index"] {
                let path = git_dir.join(rel);
                if path.exists() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }

            // Common git files.
            let packed_refs = common_git_dir.join("packed-refs");
            if packed_refs.exists() {
                println!("cargo:rerun-if-changed={}", packed_refs.display());
            }

            // Track the current branch ref file so commits on the same branch re-trigger builds.
            // Note: when refs are packed, the branch ref file may not exist; `packed-refs` covers it.
            let head_ref = std::process::Command::new("git")
                .current_dir(&manifest_dir)
                .args(["symbolic-ref", "-q", "HEAD"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty());
            if let Some(head_ref) = head_ref {
                let ref_path = common_git_dir.join(&head_ref);
                if ref_path.exists() {
                    println!("cargo:rerun-if-changed={}", ref_path.display());
                }

                let ref_log_path = common_git_dir.join("logs").join(&head_ref);
                if ref_log_path.exists() {
                    println!("cargo:rerun-if-changed={}", ref_log_path.display());
                }
            }
        }
    }

    let git_sha = {
        let mut cmd = std::process::Command::new("git");
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            cmd.current_dir(std::path::Path::new(&manifest_dir));
        }
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
