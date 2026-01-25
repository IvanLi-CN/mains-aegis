fn main() {
    // Embed build provenance for on-device log verification.
    //
    // This avoids guessing what was last flashed when iterating quickly on hardware bring-up.
    // Keep it dependency-free (no vergen) and best-effort (do not fail builds if git isn't available).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=FW_BUILD_PROFILE={}", profile);

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
