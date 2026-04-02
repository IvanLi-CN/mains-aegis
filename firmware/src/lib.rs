#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(all(feature = "main-vout-12v", feature = "main-vout-19v"))]
compile_error!("Select only one main firmware voltage feature: main-vout-12v or main-vout-19v.");

pub mod ina3221 {
    pub use ina3221_async::*;
}

pub mod bq25792;
pub mod bq40z50;
pub mod display_pipeline;
pub mod fan;
pub mod output_protection;
pub mod output_retry;
pub mod output_state;
pub mod tmp112;

pub mod audio;
