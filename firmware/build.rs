fn main() {
    // ESP32-S3 (Xtensa) app linking uses the esp-hal-provided linker scripts.
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");

    // Required by defmt (see https://defmt.ferrous-systems.com/setup#linker-script).
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
