#![allow(dead_code)]

pub mod fan {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/fan.rs"));
}

pub mod output_protection {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/output_protection.rs"
    ));
}

pub mod output_state {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/output_state.rs"
    ));
}
