//! Data types for INA3221 driver.

/// INA3221 channels.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Ch1,
    Ch2,
    Ch3,
}
