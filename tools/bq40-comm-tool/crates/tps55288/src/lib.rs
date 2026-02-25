//! TPS55288 Rust Driver (skeleton)
//!
//! This crate is a placeholder for the TPS55288 buck-boost converter driver.
//! Implementation will follow the `sc8815-rs` structure with no-std, optional async,
//! defmt support, and type-safe register access.

#![no_std]

pub mod data_types;
pub mod driver;
pub mod error;
pub mod registers;

pub use driver::Tps55288;
pub use error::Error;
pub use registers::DEFAULT_I2C_ADDRESS;
