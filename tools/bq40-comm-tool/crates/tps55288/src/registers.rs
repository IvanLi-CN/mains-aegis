//! Register map and constants for TPS55288.
//! Values and limits are copied from the datasheet; conversion helpers will be added later.

/// Default I2C address when MODE pin selects internal VCC + PWM (datasheet table, 0 Ω option).
pub const DEFAULT_I2C_ADDRESS: u8 = 0x74;
/// Alternate I2C address selected via MODE pin preset.
pub const ALT_I2C_ADDRESS: u8 = 0x75;

/// Register addresses (7-bit).
pub mod addr {
    /// Reference voltage DAC (LSB 20 mV). Addresses 0x00 and 0x01 hold the 10-bit value.
    pub const REF0: u8 = 0x00;
    pub const REF1: u8 = 0x01;
    /// Output current limit (50 mA LSB, ~6.35 A max)
    pub const IOUT_LIMIT: u8 = 0x02;
    /// VOUT slew-rate control
    pub const VOUT_SR: u8 = 0x03;
    /// Feedback selection (internal DAC vs external divider)
    pub const VOUT_FS: u8 = 0x04;
    /// Cable droop compensation
    pub const CDC: u8 = 0x05;
    /// Mode control (PFM/FPWM, VCC source, I2C address select, hiccup, discharge, FSW double, OE)
    pub const MODE: u8 = 0x06;
    /// Operating/fault status (write-1-to-clear fields)
    pub const STATUS: u8 = 0x07;
}

/// Voltage DAC characteristics.
pub const VOUT_LSB_MV: u16 = 20;
pub const VOUT_MIN_MV: u16 = 800;
// 10-bit DAC => 1024 steps from 0 to 1023 inclusive.
pub const VOUT_MAX_MV: u16 = VOUT_MIN_MV + (1023 * VOUT_LSB_MV);

/// Output current limit DAC characteristics.
pub const ILIM_LSB_MA: u16 = 50;
pub const ILIM_MAX_MA: u16 = 6_350;

/// Switching frequency range (set by RFSW pin; register FSWDBL in MODE can double frequency).
pub const FSW_MIN_KHZ: u16 = 200;
pub const FSW_MAX_KHZ: u16 = 2_200;

bitflags::bitflags! {
    /// IOUT_LIMIT register bits (0x02).
    pub struct IoutLimitBits: u8 {
        /// Bit 7: Current limit enable (1 = enabled).
        const EN         = 1 << 7;
        /// Bits 0-6: Current limit setting (steps of 0.5 mV across sense resistor).
        const SETTING_LSB = 1 << 0;
    }

    /// MODE register bits (0x06).
    pub struct ModeBits: u8 {
        /// Bit 7: Output enable.
        const OE      = 1 << 7;
        /// Bit 6: Double switching frequency.
        const FSWDBL  = 1 << 6;
        /// Bit 5: Hiccup protection enable.
        const HICCUP  = 1 << 5;
        /// Bit 4: Output discharge enable.
        const DISCHG  = 1 << 4;
        /// Bit 3: VCC selection (0 = internal LDO, 1 = external 5 V).
        const VCC_EXT = 1 << 3;
        /// Bit 2: I2C address select (0 = 0x74, 1 = 0x75) when not overridden by MODE resistor.
        const I2CADD  = 1 << 2;
        /// Bit 1: Light-load mode at light load (0 = PFM, 1 = FPWM).
        const PFM     = 1 << 1;
        /// Bit 0: Operating mode selection (datasheet-defined behavior; keep for completeness).
        const MODE    = 1 << 0;
    }

    /// STATUS register bits (0x07). Bits 7-5 are fault indicators; bits 1-0 encode operating mode.
    pub struct StatusBits: u8 {
        const SCP      = 1 << 7;
        const OCP      = 1 << 6;
        const OVP      = 1 << 5;
        // Bits 4-2 reserved.
        const STATUS0  = 1 << 0;
        const STATUS1  = 1 << 1;
    }

    /// CDC register bits (0x05) masks and options.
    pub struct CdcBits: u8 {
        const SC_MASK   = 1 << 7;
        const OCP_MASK  = 1 << 6;
        const OVP_MASK  = 1 << 5;
        // Bit 4 reserved.
        const CDC_OPT   = 1 << 3;
        const CDC0      = 1 << 0;
        const CDC1      = 1 << 1;
        const CDC2      = 1 << 2;
    }

    /// VOUT_FS register bits (0x04).
    pub struct VoutFsBits: u8 {
        /// Bit 7: Select internal (0) vs external (1) feedback divider.
        const FB_EXT   = 1 << 7;
        /// Bits 1-0: Internal feedback ratio (when FB_EXT = 0).
        const INTFB0   = 1 << 0;
        const INTFB1   = 1 << 1;
    }

    /// VOUT_SR register bits (0x03).
    pub struct VoutSrBits: u8 {
        // Bits 7-6 reserved.
        const OCP_DELAY0 = 1 << 4;
        const OCP_DELAY1 = 1 << 5;
        // Bits 3-2 reserved.
        const SR0        = 1 << 0;
        const SR1        = 1 << 1;
    }
}

/// Convert VOUT millivolts to DAC code (10-bit, 20 mV LSB). Clamps to datasheet limits.
pub fn vout_mv_to_code(mv: u16) -> u16 {
    let mv = mv.clamp(VOUT_MIN_MV, VOUT_MAX_MV);
    let code = mv.saturating_sub(VOUT_MIN_MV) / VOUT_LSB_MV;
    code.min(1023)
}

/// Convert DAC code to VOUT millivolts.
pub fn code_to_vout_mv(code: u16) -> u16 {
    let code = code.min(1023);
    VOUT_MIN_MV + code * VOUT_LSB_MV
}

/// Convert output current limit (mA) to DAC code (50 mA LSB). Clamps to datasheet max.
pub fn ilim_ma_to_code(ma: u16) -> u8 {
    let ma = ma.min(ILIM_MAX_MA);
    (ma / ILIM_LSB_MA).min(ILIM_MAX_MA / ILIM_LSB_MA) as u8
}

/// Convert current limit DAC code to milliamps.
pub fn code_to_ilim_ma(code: u8) -> u16 {
    let code = (code as u16).min(ILIM_MAX_MA / ILIM_LSB_MA);
    code * ILIM_LSB_MA
}

// TODO: confirm MODE bit0 semantics when implementing driver.

/// Decode STATUS operating status bits into mode index (0b00 boost, 0b01 buck, 0b10 buck-boost, 0b11 reserved).
pub fn decode_status_mode(bits: &StatusBits) -> u8 {
    let raw = bits.bits() & (StatusBits::STATUS0 | StatusBits::STATUS1).bits();
    raw & 0b11
}
