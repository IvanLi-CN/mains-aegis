//! Minimal BQ25792 helpers (bring-up oriented).
//!
//! This module intentionally keeps a thin surface:
//! - raw register read/write helpers
//! - a few bit definitions for the status/control registers we care about
//! - basic decoding helpers for log output
//!
//! Datasheet reference: `docs/datasheets/BQ25792/BQ25792.md`

/// 7-bit I2C address (datasheet: 0x6B).
pub const I2C_ADDRESS: u8 = 0x6B;

pub mod reg {
    // 16-bit register LSB addresses.
    pub const CHARGE_CURRENT_LIMIT: u8 = 0x03;
    pub const INPUT_CURRENT_LIMIT: u8 = 0x06;

    pub const CHARGER_CONTROL_0: u8 = 0x0F;
    pub const CHARGER_CONTROL_2: u8 = 0x11;
    pub const CHARGER_CONTROL_5: u8 = 0x14;

    pub const CHARGER_STATUS_0: u8 = 0x1B;
    pub const CHARGER_STATUS_1: u8 = 0x1C;
    pub const CHARGER_STATUS_2: u8 = 0x1D;
    pub const CHARGER_STATUS_3: u8 = 0x1E;
    pub const CHARGER_STATUS_4: u8 = 0x1F;

    pub const FAULT_STATUS_0: u8 = 0x20;
    pub const FAULT_STATUS_1: u8 = 0x21;
    pub const ADC_CONTROL: u8 = 0x2E;
    pub const VBAT_ADC: u8 = 0x3B;
    pub const VSYS_ADC: u8 = 0x3D;
}

pub mod ctrl0 {
    /// `REGOF_Charger_Control_0.EN_CHG` (bit 5).
    pub const EN_CHG: u8 = 1 << 5;
    /// `REGOF_Charger_Control_0.EN_HIZ` (bit 2).
    pub const EN_HIZ: u8 = 1 << 2;
}

pub mod ctrl2 {
    /// `REG11.Charger_Control_2.SDRV_CTRL[1:0]` lives at bits 2:1.
    pub const SDRV_CTRL_SHIFT: u8 = 1;
    pub const SDRV_CTRL_MASK: u8 = 0b11 << SDRV_CTRL_SHIFT;
}

pub mod ctrl5 {
    /// `REG14.SFET_PRESENT` (bit 7).
    pub const SFET_PRESENT: u8 = 1 << 7;
}

pub mod status0 {
    pub const IINDPM_STAT: u8 = 1 << 7;
    pub const VINDPM_STAT: u8 = 1 << 6;
    pub const WD_STAT: u8 = 1 << 5;
    pub const POORSRC_STAT: u8 = 1 << 4;
    pub const PG_STAT: u8 = 1 << 3;
    pub const AC2_PRESENT_STAT: u8 = 1 << 2;
    pub const AC1_PRESENT_STAT: u8 = 1 << 1;
    pub const VBUS_PRESENT_STAT: u8 = 1 << 0;
}

pub mod status1 {
    /// Extract `CHG_STAT_2:0` from `REG1C` (bits 7..=5).
    pub const fn chg_stat(reg1c: u8) -> u8 {
        (reg1c >> 5) & 0x07
    }

    /// Extract `VBUS_STAT_3:0` from `REG1C` (bits 4..=1).
    pub const fn vbus_stat(reg1c: u8) -> u8 {
        (reg1c >> 1) & 0x0F
    }

    pub const BC12_DONE_STAT: u8 = 1 << 0;
}

pub mod status2 {
    /// Extract `ICO_STAT_1:0` from `REG1D` (bits 7..=6).
    pub const fn ico_stat(reg1d: u8) -> u8 {
        (reg1d >> 6) & 0x03
    }

    pub const TREG_STAT: u8 = 1 << 2;
    pub const DPDM_STAT: u8 = 1 << 1;
    pub const VBAT_PRESENT_STAT: u8 = 1 << 0;
}

pub mod status4 {
    pub const VBATOTG_LOW_STAT: u8 = 1 << 4;
    pub const TS_COLD_STAT: u8 = 1 << 3;
    pub const TS_COOL_STAT: u8 = 1 << 2;
    pub const TS_WARM_STAT: u8 = 1 << 1;
    pub const TS_HOT_STAT: u8 = 1 << 0;
}

pub mod status3 {
    pub const ACRB2_STAT: u8 = 1 << 7;
    pub const ACRB1_STAT: u8 = 1 << 6;
    pub const ADC_DONE_STAT: u8 = 1 << 5;
    pub const VSYS_STAT: u8 = 1 << 4;
    pub const CHG_TMR_STAT: u8 = 1 << 3;
    pub const TRICHG_TMR_STAT: u8 = 1 << 2;
}

pub mod adc_ctrl {
    pub const ADC_EN: u8 = 1 << 7;
}

pub fn read_u8<I2C>(i2c: &mut I2C, reg: u8) -> Result<u8, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 1];
    i2c.write_read(I2C_ADDRESS, &[reg], &mut buf)?;
    Ok(buf[0])
}

pub fn read_block<I2C>(i2c: &mut I2C, start_reg: u8, buf: &mut [u8]) -> Result<(), I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    i2c.write_read(I2C_ADDRESS, &[start_reg], buf)
}

pub fn read_u16<I2C>(i2c: &mut I2C, reg_lsb: u8) -> Result<u16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 2];
    i2c.write_read(I2C_ADDRESS, &[reg_lsb], &mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub fn write_u8<I2C>(i2c: &mut I2C, reg: u8, value: u8) -> Result<(), I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    i2c.write(I2C_ADDRESS, &[reg, value])
}

pub fn write_u16<I2C>(i2c: &mut I2C, reg_lsb: u8, value: u16) -> Result<(), I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let [lsb, msb] = value.to_le_bytes();
    i2c.write(I2C_ADDRESS, &[reg_lsb, lsb, msb])
}

/// Read-modify-write a single 8-bit register.
///
/// Returns the value that the function attempted to apply.
pub fn update_u8<I2C>(
    i2c: &mut I2C,
    reg: u8,
    clear_mask: u8,
    set_mask: u8,
) -> Result<u8, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let cur = read_u8(i2c, reg)?;
    let new = (cur & !clear_mask) | set_mask;
    if new != cur {
        write_u8(i2c, reg, new)?;
    }
    Ok(new)
}

fn clamp_u16(value: u16, min: u16, max: u16) -> u16 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

pub fn set_charge_current_limit_ma<I2C>(i2c: &mut I2C, ma: u16) -> Result<u16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    // ICHG range: 50mA..=5000mA, step 10mA.
    const MIN_MA: u16 = 50;
    const MAX_MA: u16 = 5000;
    const FIELD_MASK: u16 = 0x01FF;

    let ma = clamp_u16(ma, MIN_MA, MAX_MA);
    let field = (ma / 10) & FIELD_MASK;

    let cur = read_u16(i2c, reg::CHARGE_CURRENT_LIMIT)?;
    let new = (cur & !FIELD_MASK) | field;
    if new != cur {
        write_u16(i2c, reg::CHARGE_CURRENT_LIMIT, new)?;
    }
    Ok(new)
}

pub fn set_input_current_limit_ma<I2C>(i2c: &mut I2C, ma: u16) -> Result<u16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    // IINDPM range: 100mA..=3300mA, step 10mA.
    const MIN_MA: u16 = 100;
    const MAX_MA: u16 = 3300;
    const FIELD_MASK: u16 = 0x01FF;

    let ma = clamp_u16(ma, MIN_MA, MAX_MA);
    let field = (ma / 10) & FIELD_MASK;

    let cur = read_u16(i2c, reg::INPUT_CURRENT_LIMIT)?;
    let new = (cur & !FIELD_MASK) | field;
    if new != cur {
        write_u16(i2c, reg::INPUT_CURRENT_LIMIT, new)?;
    }
    Ok(new)
}

#[derive(Clone, Copy)]
pub struct ShipFetState {
    pub ctrl2_before: u8,
    pub ctrl2_after: u8,
    pub sdrv_ctrl_before: u8,
    pub sdrv_ctrl_after: u8,
}

/// Force SDRV control into IDLE (00) so external ship FET is not left off.
pub fn ensure_ship_fet_idle<I2C>(i2c: &mut I2C) -> Result<ShipFetState, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let ctrl2_before = read_u8(i2c, reg::CHARGER_CONTROL_2)?;
    let sdrv_ctrl_before = (ctrl2_before & ctrl2::SDRV_CTRL_MASK) >> ctrl2::SDRV_CTRL_SHIFT;
    let ctrl2_after = ctrl2_before & !ctrl2::SDRV_CTRL_MASK;
    if ctrl2_after != ctrl2_before {
        write_u8(i2c, reg::CHARGER_CONTROL_2, ctrl2_after)?;
    }
    let sdrv_ctrl_after = (ctrl2_after & ctrl2::SDRV_CTRL_MASK) >> ctrl2::SDRV_CTRL_SHIFT;

    Ok(ShipFetState {
        ctrl2_before,
        ctrl2_after,
        sdrv_ctrl_before,
        sdrv_ctrl_after,
    })
}

#[derive(Clone, Copy)]
pub struct ShipFetPathState {
    pub ctrl5_before: u8,
    pub ctrl5_after: u8,
    pub ship: ShipFetState,
}

/// Ensure ship-FET feature is enabled and SDRV stays in IDLE (00).
pub fn ensure_ship_fet_path_enabled<I2C>(i2c: &mut I2C) -> Result<ShipFetPathState, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let ctrl5_before = read_u8(i2c, reg::CHARGER_CONTROL_5)?;
    let ctrl5_after = ctrl5_before | ctrl5::SFET_PRESENT;
    if ctrl5_after != ctrl5_before {
        write_u8(i2c, reg::CHARGER_CONTROL_5, ctrl5_after)?;
    }

    let ship = ensure_ship_fet_idle(i2c)?;
    Ok(ShipFetPathState {
        ctrl5_before,
        ctrl5_after,
        ship,
    })
}

pub const fn decode_chg_stat(code: u8) -> &'static str {
    match code & 0x07 {
        0 => "not_charging",
        1 => "trickle",
        2 => "precharge",
        3 => "fast_cc",
        4 => "taper_cv",
        5 => "reserved",
        6 => "topoff_timer",
        7 => "termination_done",
        _ => "reserved",
    }
}

pub const fn decode_vbus_stat(code: u8) -> &'static str {
    match code & 0x0F {
        0x0 => "no_input",
        0x1 => "usb_sdp_500ma",
        0x2 => "usb_cdp_1p5a",
        0x3 => "usb_dcp_3p25a",
        0x4 => "hvdcp_1p5a",
        0x5 => "unknown_adapter_3a",
        0x6 => "nonstandard_adapter",
        0x7 => "otg_mode",
        0x8 => "not_qualified_adapter",
        0xB => "powered_from_vbus",
        _ => "reserved",
    }
}

pub const fn decode_ico_stat(code: u8) -> &'static str {
    match code & 0x03 {
        0 => "disabled",
        1 => "in_progress",
        2 => "max_detected",
        _ => "reserved",
    }
}
