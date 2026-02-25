//! Error definitions for TPS55288 driver.

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum Error<I2cError> {
    /// Underlying I2C transaction failed.
    I2c(I2cError),
    /// Provided parameter was outside datasheet limits.
    OutOfRange,
    /// Unsupported/invalid configuration for current mode.
    InvalidConfig,
}

impl<I2cError: core::fmt::Debug> core::fmt::Display for Error<I2cError> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::I2c(e) => write!(f, "I2C error: {:?}", e),
            Error::OutOfRange => write!(f, "parameter out of range"),
            Error::InvalidConfig => write!(f, "invalid configuration for current mode"),
        }
    }
}
