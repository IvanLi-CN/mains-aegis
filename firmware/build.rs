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
            for rel in ["HEAD", "logs/HEAD", "packed-refs", "index"] {
                let path = git_dir.join(rel);
                if path.exists() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }

    let git_sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
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
        .unwrap_or_else(|| "unknown".to_string());
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
