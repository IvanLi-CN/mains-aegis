fn main() {
    println!("cargo::rustc-check-cfg=cfg(codex_host_test)");
    println!("cargo::rustc-cfg=codex_host_test");
}
