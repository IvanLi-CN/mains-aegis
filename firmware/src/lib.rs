#![no_std]

#[cfg(feature = "net_http")]
extern crate alloc;

#[cfg(test)]
extern crate std;

#[cfg(all(feature = "main-vout-12v", feature = "main-vout-19v"))]
compile_error!("Select only one main firmware voltage feature: main-vout-12v or main-vout-19v.");

#[cfg(all(
    not(feature = "no-pps"),
    not(any(
        not(feature = "no-pd-sink-5v"),
        not(feature = "no-pd-sink-9v"),
        not(feature = "no-pd-sink-12v"),
        not(feature = "no-pd-sink-15v"),
        not(feature = "no-pd-sink-20v")
    ))
))]
compile_error!(
    "PPS requires at least one enabled fixed PDO; clear at least one no-pd-sink-* feature."
);

pub mod time;

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
pub mod usb_pd;

#[cfg(feature = "net_http")]
pub mod mdns;
pub mod mdns_wire;
#[cfg(feature = "net_http")]
pub mod net;
pub mod net_contract;
pub mod net_types;

pub mod audio;
