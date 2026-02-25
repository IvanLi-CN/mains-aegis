//! Driver scaffold for INA3221.
//! Provides blocking I2C helpers; async version mirrors this API behind the `async` feature.

use crate::data_types::Channel;
use crate::error::Error;
use crate::registers::{
    addr, decode_shift3_signed_i16, DEFAULT_I2C_ADDRESS, VBUS_LSB_MV, VSHUNT_LSB_UV,
};

/// INA3221 driver.
pub struct Ina3221<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C> Ina3221<I2C> {
    /// Create a new driver instance with default address (0x40).
    pub fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            address: DEFAULT_I2C_ADDRESS,
        }
    }

    /// Create a new driver instance with a custom I2C address.
    pub fn with_address(i2c: I2C, address: u8) -> Self {
        Self { i2c, address }
    }

    /// Return the 7-bit I2C address configured for this instance.
    pub fn address(&self) -> u8 {
        self.address
    }

    /// Update the I2C address used by this instance.
    pub fn set_address(&mut self, address: u8) {
        self.address = address;
    }

    /// Consume the driver and return the underlying I2C bus.
    pub fn free(self) -> I2C {
        self.i2c
    }
}

fn ch_shunt_reg(ch: Channel) -> u8 {
    match ch {
        Channel::Ch1 => addr::CH1_SHUNT,
        Channel::Ch2 => addr::CH2_SHUNT,
        Channel::Ch3 => addr::CH3_SHUNT,
    }
}

fn ch_bus_reg(ch: Channel) -> u8 {
    match ch {
        Channel::Ch1 => addr::CH1_BUS,
        Channel::Ch2 => addr::CH2_BUS,
        Channel::Ch3 => addr::CH3_BUS,
    }
}

/// Convert shunt voltage (µV) to current (mA) using `Rshunt` in milli-ohms.
pub fn shunt_uv_to_current_ma(shunt_uv: i32, rshunt_mohm: i32) -> i32 {
    // I(mA) = Vshunt(µV) / Rshunt(mΩ)
    if rshunt_mohm <= 0 {
        return 0;
    }
    shunt_uv / rshunt_mohm
}

#[cfg(not(feature = "async"))]
impl<I2C> Ina3221<I2C>
where
    I2C: embedded_hal::i2c::I2c,
{
    /// Write a 16-bit register value (big-endian).
    pub fn write_reg_u16_be(&mut self, reg: u8, value: u16) -> Result<(), Error<I2C::Error>> {
        let bytes = value.to_be_bytes();
        self.i2c
            .write(self.address, &[reg, bytes[0], bytes[1]])
            .map_err(Error::I2c)
    }

    /// Read a 16-bit register value (big-endian).
    pub fn read_reg_u16_be(&mut self, reg: u8) -> Result<u16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg], &mut buf)
            .map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    /// Read a signed `i16` register value and right-shift by 3 (INA3221 data alignment).
    pub fn read_reg_i16_shift3_be(&mut self, reg: u8) -> Result<i16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg], &mut buf)
            .map_err(Error::I2c)?;
        Ok(decode_shift3_signed_i16(buf))
    }

    /// Set CONFIG register.
    pub fn set_config(&mut self, config: u16) -> Result<(), Error<I2C::Error>> {
        self.write_reg_u16_be(addr::CONFIG, config)
    }

    /// Read CONFIG register.
    pub fn read_config(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::CONFIG)
    }

    /// Read bus voltage (mV) for a channel.
    pub fn read_bus_mv(&mut self, ch: Channel) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_reg_i16_shift3_be(ch_bus_reg(ch))?;
        Ok((raw as i32) * VBUS_LSB_MV)
    }

    /// Read shunt voltage (µV) for a channel.
    pub fn read_shunt_uv(&mut self, ch: Channel) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_reg_i16_shift3_be(ch_shunt_reg(ch))?;
        Ok((raw as i32) * VSHUNT_LSB_UV)
    }

    /// Read manufacturer ID register.
    pub fn read_manufacturer_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::MANUFACTURER_ID)
    }

    /// Read die ID register.
    pub fn read_die_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::DIE_ID)
    }
}

#[cfg(not(feature = "async"))]
pub fn init_with_config<I2C: embedded_hal::i2c::I2c>(
    i2c: &mut I2C,
    config: u16,
) -> Result<(), Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.set_config(config)
}

#[cfg(not(feature = "async"))]
pub fn read_bus_mv<I2C: embedded_hal::i2c::I2c>(
    i2c: &mut I2C,
    ch: Channel,
) -> Result<i32, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_bus_mv(ch)
}

#[cfg(not(feature = "async"))]
pub fn read_shunt_uv<I2C: embedded_hal::i2c::I2c>(
    i2c: &mut I2C,
    ch: Channel,
) -> Result<i32, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_shunt_uv(ch)
}

#[cfg(not(feature = "async"))]
pub fn read_config<I2C: embedded_hal::i2c::I2c>(i2c: &mut I2C) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_config()
}

#[cfg(not(feature = "async"))]
pub fn read_manufacturer_id<I2C: embedded_hal::i2c::I2c>(
    i2c: &mut I2C,
) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_manufacturer_id()
}

#[cfg(not(feature = "async"))]
pub fn read_die_id<I2C: embedded_hal::i2c::I2c>(i2c: &mut I2C) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_die_id()
}

#[cfg(feature = "async")]
impl<I2C> Ina3221<I2C>
where
    I2C: embedded_hal_async::i2c::I2c,
{
    pub async fn write_reg_u16_be(&mut self, reg: u8, value: u16) -> Result<(), Error<I2C::Error>> {
        let bytes = value.to_be_bytes();
        self.i2c
            .write(self.address, &[reg, bytes[0], bytes[1]])
            .await
            .map_err(Error::I2c)
    }

    pub async fn read_reg_u16_be(&mut self, reg: u8) -> Result<u16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg], &mut buf)
            .await
            .map_err(Error::I2c)?;
        Ok(u16::from_be_bytes(buf))
    }

    pub async fn read_reg_i16_shift3_be(&mut self, reg: u8) -> Result<i16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg], &mut buf)
            .await
            .map_err(Error::I2c)?;
        Ok(decode_shift3_signed_i16(buf))
    }

    pub async fn set_config(&mut self, config: u16) -> Result<(), Error<I2C::Error>> {
        self.write_reg_u16_be(addr::CONFIG, config).await
    }

    pub async fn read_config(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::CONFIG).await
    }

    pub async fn read_bus_mv(&mut self, ch: Channel) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_reg_i16_shift3_be(ch_bus_reg(ch)).await?;
        Ok((raw as i32) * VBUS_LSB_MV)
    }

    pub async fn read_shunt_uv(&mut self, ch: Channel) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_reg_i16_shift3_be(ch_shunt_reg(ch)).await?;
        Ok((raw as i32) * VSHUNT_LSB_UV)
    }

    pub async fn read_manufacturer_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::MANUFACTURER_ID).await
    }

    pub async fn read_die_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_reg_u16_be(addr::DIE_ID).await
    }
}

#[cfg(feature = "async")]
pub async fn init_with_config<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
    config: u16,
) -> Result<(), Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.set_config(config).await
}

#[cfg(feature = "async")]
pub async fn read_bus_mv<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
    ch: Channel,
) -> Result<i32, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_bus_mv(ch).await
}

#[cfg(feature = "async")]
pub async fn read_shunt_uv<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
    ch: Channel,
) -> Result<i32, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_shunt_uv(ch).await
}

#[cfg(feature = "async")]
pub async fn read_config<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_config().await
}

#[cfg(feature = "async")]
pub async fn read_manufacturer_id<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_manufacturer_id().await
}

#[cfg(feature = "async")]
pub async fn read_die_id<I2C: embedded_hal_async::i2c::I2c>(
    i2c: &mut I2C,
) -> Result<u16, Error<I2C::Error>> {
    let mut dev = Ina3221::new(i2c);
    dev.read_die_id().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registers::decode_shift3_signed_i16;

    #[test]
    fn decode_bus_mv_example() {
        // Example: vbus_mv=4984 => raw_bus = 4984/8 = 623
        // Register stores raw in bits 14..=3 => word = 623 << 3 = 4984 (0x1378)
        let raw = decode_shift3_signed_i16([0x13, 0x78]);
        assert_eq!(raw, 623);
        assert_eq!((raw as i32) * VBUS_LSB_MV, 4984);
    }

    #[test]
    fn decode_shunt_and_current_example() {
        // current_ma=312 @ 10mΩ => vshunt_uv=312*10=3120uV
        // raw_shunt = 3120/40 = 78
        // reg word = 78 << 3 = 624 (0x0270)
        let raw = decode_shift3_signed_i16([0x02, 0x70]);
        assert_eq!(raw, 78);
        let shunt_uv = (raw as i32) * VSHUNT_LSB_UV;
        assert_eq!(shunt_uv, 3120);
        assert_eq!(shunt_uv_to_current_ma(shunt_uv, 10), 312);
    }

    #[test]
    fn decode_negative_shunt() {
        // -1 LSB after shift => word = -8 (0xFFF8)
        let raw = decode_shift3_signed_i16([0xFF, 0xF8]);
        assert_eq!(raw, -1);
        assert_eq!((raw as i32) * VSHUNT_LSB_UV, -40);
    }
}
