use embedded_hal::i2c::I2c;

const REG_TEMP: u8 = 0x00;
const REG_CONFIG: u8 = 0x01;
const REG_TLOW: u8 = 0x02;
const REG_THIGH: u8 = 0x03;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultQueue {
    F1 = 1,
    F2 = 2,
    F4 = 4,
    F6 = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionRate {
    Hz0_25,
    Hz1,
    Hz4,
    Hz8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertConfig {
    pub t_high_c_x16: i16,
    pub t_low_c_x16: i16,
    pub fault_queue: FaultQueue,
    pub conversion_rate: ConversionRate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertConfigReadback {
    pub config: u16,
    pub tlow: u16,
    pub thigh: u16,
}

pub fn program_alert_config<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cfg: AlertConfig,
) -> Result<AlertConfigReadback, I2C::Error>
where
    I2C: I2c,
{
    write_reg_u16(i2c, addr, REG_CONFIG, encode_config_reg(cfg))?;
    write_reg_u16(i2c, addr, REG_TLOW, encode_temp_reg(cfg.t_low_c_x16))?;
    write_reg_u16(i2c, addr, REG_THIGH, encode_temp_reg(cfg.t_high_c_x16))?;

    // Read back for bring-up verification.
    Ok(AlertConfigReadback {
        config: read_reg_u16(i2c, addr, REG_CONFIG)?,
        tlow: read_reg_u16(i2c, addr, REG_TLOW)?,
        thigh: read_reg_u16(i2c, addr, REG_THIGH)?,
    })
}

pub fn read_alert_config_readback<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<AlertConfigReadback, I2C::Error>
where
    I2C: I2c,
{
    Ok(AlertConfigReadback {
        config: read_reg_u16(i2c, addr, REG_CONFIG)?,
        tlow: read_reg_u16(i2c, addr, REG_TLOW)?,
        thigh: read_reg_u16(i2c, addr, REG_THIGH)?,
    })
}

fn write_reg_u16<I2C>(i2c: &mut I2C, addr: u8, reg: u8, value: u16) -> Result<(), I2C::Error>
where
    I2C: I2c,
{
    let [msb, lsb] = value.to_be_bytes();
    i2c.write(addr, &[reg, msb, lsb])
}

fn read_reg_u16<I2C>(i2c: &mut I2C, addr: u8, reg: u8) -> Result<u16, I2C::Error>
where
    I2C: I2c,
{
    let mut buf = [0u8; 2];
    i2c.write_read(addr, &[reg], &mut buf)?;
    Ok(u16::from_be_bytes(buf))
}

pub const fn encode_temp_reg(temp_c_x16: i16) -> u16 {
    ((temp_c_x16 as u16) << 4) & 0xFFF0
}

pub const fn encode_config_reg(cfg: AlertConfig) -> u16 {
    // TMP112 config register is 16-bit, MSB first.
    //
    // Byte1: OS R1 R0 F1 F0 POL TM SD
    // Byte2: CR1 CR0 AL EM 0 0 0 0
    //
    // We force: comparator mode (TM=0), active-low (POL=0), continuous (SD=0).
    // R1/R0 are read-only and typically read back as 1. We write them as 1 for clarity.
    let mut b1: u8 = 0x60; // OS=0, R1/R0=1/1, F1/F0/POL/TM/SD=0
    b1 |= match cfg.fault_queue {
        FaultQueue::F1 => 0x00, // F1/F0 = 0/0
        FaultQueue::F2 => 0x08, // 0/1
        FaultQueue::F4 => 0x10, // 1/0
        FaultQueue::F6 => 0x18, // 1/1
    };

    let mut b2: u8 = 0x00; // EM=0 (normal mode)
    b2 |= match cfg.conversion_rate {
        ConversionRate::Hz0_25 => 0x00, // CR1/CR0 = 0/0
        ConversionRate::Hz1 => 0x40,    // 0/1
        ConversionRate::Hz4 => 0x80,    // 1/0
        ConversionRate::Hz8 => 0xC0,    // 1/1
    };

    u16::from_be_bytes([b1, b2])
}

#[cfg(test)]
mod tests {
    use super::{
        decode_temp_c_x16, encode_config_reg, encode_temp_reg, AlertConfig, ConversionRate,
        FaultQueue,
    };

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

    #[test]
    fn encode_temp_reg_positive() {
        // 50°C => 50*16=800 => 0x0320 << 4 => 0x3200
        assert_eq!(encode_temp_reg(800), 0x3200);
    }

    #[test]
    fn encode_temp_reg_negative() {
        // -1°C => -16 (0xFFF0) shifted into the register format => 0xFF00.
        assert_eq!(encode_temp_reg(-16), 0xFF00);
    }

    #[test]
    fn encode_config_fault_queue_and_rate() {
        let cfg = AlertConfig {
            t_high_c_x16: 800,
            t_low_c_x16: 640,
            fault_queue: FaultQueue::F4,
            conversion_rate: ConversionRate::Hz1,
        };
        // b1=0x60 | 0x10 (F4), b2=0x40 (1 Hz)
        assert_eq!(encode_config_reg(cfg), 0x7040);
    }
}
