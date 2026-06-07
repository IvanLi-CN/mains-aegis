use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let web_dist = manifest_dir.join("../../web/dist");

    println!("cargo:rerun-if-changed={}", web_dist.display());
    println!("cargo:rerun-if-changed=../../web/index.html");
    println!("cargo:rerun-if-changed=../../web/src");
    println!("cargo:rerun-if-changed=../../web/public");

    let index_html = web_dist.join("index.html");
    if !index_html.is_file() {
        panic!(
            "embedded Web assets are missing: expected {}. Run `bun run web:build` before building tools/mains-aegis-host.",
            index_html.display()
        );
    }

    println!(
        "cargo:rustc-env=MAINS_AEGIS_EMBEDDED_WEB_DIST={}",
        web_dist.display()
    );
}
