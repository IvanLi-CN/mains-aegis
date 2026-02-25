//! INA3221 Rust Driver
//!
//! Minimal driver for TI INA3221 3-channel current/voltage monitor.
//! Provides blocking I2C helpers and an optional async API behind the `async` feature.

#![no_std]

pub mod data_types;
pub mod driver;
pub mod error;
pub mod registers;

pub use data_types::Channel;
pub use driver::Ina3221;
pub use error::Error;
pub use registers::DEFAULT_I2C_ADDRESS;
pub use registers::{
    CONFIG_VALUE_CH12, CONFIG_VALUE_CH123, CONFIG_VALUE_CH123_STABLE, CONFIG_VALUE_CH12_STABLE,
    VBUS_LSB_MV, VSHUNT_LSB_UV,
};

#[cfg(not(feature = "async"))]
pub use driver::{
    init_with_config, read_bus_mv, read_config, read_die_id, read_manufacturer_id, read_shunt_uv,
    shunt_uv_to_current_ma,
};

#[cfg(feature = "async")]
pub use driver::{
    init_with_config, read_bus_mv, read_config, read_die_id, read_manufacturer_id, read_shunt_uv,
    shunt_uv_to_current_ma,
};
