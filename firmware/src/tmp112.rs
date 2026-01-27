use embedded_hal::i2c::I2c;

const REG_TEMP: u8 = 0x00;

pub fn read_temp_c_x16<I2C>(i2c: &mut I2C, addr: u8) -> Result<i16, I2C::Error>
where
    I2C: I2c,
{
    let mut buf = [0u8; 2];
    i2c.write_read(addr, &[REG_TEMP], &mut buf)?;
    Ok(decode_temp_c_x16(u16::from_be_bytes(buf)))
}

pub const fn decode_temp_c_x16(raw: u16) -> i16 {
    (raw as i16) >> 4
}

#[cfg(test)]
mod tests {
    use super::decode_temp_c_x16;

    #[test]
    fn decode_positive() {
        assert_eq!(decode_temp_c_x16(0x1900), 0x0190);
    }

    #[test]
    fn decode_negative() {
        assert_eq!(decode_temp_c_x16(0xFF00), -16);
    }

    #[test]
    fn decode_zero() {
        assert_eq!(decode_temp_c_x16(0x0000), 0);
    }
}
