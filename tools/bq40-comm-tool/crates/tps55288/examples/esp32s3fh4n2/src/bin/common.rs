#![allow(dead_code)]

use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    time::Rate,
    Blocking,
};
use esp_println::println;

// Required by espflash (ESP-IDF image format): provides the app descriptor section.
esp_bootloader_esp_idf::esp_app_desc!();

use tps55288::data_types::{
    CableCompLevel, CableCompOption, FaultStatus, FeedbackSource, InternalFeedbackRatio, OcpDelay,
    OperatingStatus, VoutSlewRate,
};
use tps55288::driver::Tps55288;
use tps55288::registers::{addr, ModeBits};

// Pin mapping (ESP32-S3 QFN56):
// - pin 42 = GPIO37 (wired to CE through an inverter)
// - pin 43 = GPIO38 (INT)
// - pin 44 = GPIO39 (SDA, MTCK)
// - pin 45 = GPIO40 (SCL, MTDO)

pub type BoardI2c = I2c<'static, Blocking>;

pub struct Board {
    pub i2c: BoardI2c,
    /// CE is hardware-inverted: set LOW to enable TPS55288.
    pub ce_inverted: Output<'static>,
    pub int: Input<'static>,
    pub delay: Delay,
}

pub fn init_board() -> Board {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let delay = Delay::new();

    let i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    )
    .unwrap()
    .with_sda(peripherals.GPIO39) // SDA, MCU pin 44
    .with_scl(peripherals.GPIO40); // SCL, MCU pin 45

    // Start disabled (because CE is inverted on the board).
    let ce_inverted = Output::new(peripherals.GPIO37, Level::High, OutputConfig::default());

    let int = Input::new(
        peripherals.GPIO38,
        InputConfig::default().with_pull(Pull::Up),
    );

    Board {
        i2c,
        ce_inverted,
        int,
        delay,
    }
}

/// Apply a safe baseline configuration using internal DAC feedback (FB pin unused).
pub fn setup_device<I2C>(dev: &mut Tps55288<I2C>)
where
    I2C: embedded_hal::i2c::I2c,
{
    println!("Configuring TPS55288 with internal DAC feedback (OE disabled)");

    if let Err(e) = dev.init() {
        println!("init failed: {:?}", e);
    }
    if let Err(e) = dev.set_ilim_ma(3_000, true) {
        println!("set_ilim failed: {:?}", e);
    }
    // Use the smallest internal divider (R0_0564) so the REF DAC maps 0.8–21 V correctly.
    if let Err(e) = dev.set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564) {
        println!("set_feedback failed: {:?}", e);
    }
    if let Err(e) = dev.set_cable_comp(
        CableCompOption::Internal,
        CableCompLevel::V0p0,
        true,
        true,
        true,
    ) {
        println!("set_cable_comp failed: {:?}", e);
    }
    if let Err(e) = dev.set_vout_sr(VoutSlewRate::Sr2p5MvPerUs, OcpDelay::Us128) {
        println!("set_vout_sr failed: {:?}", e);
    }

    // Force FPWM at light load using MODE register:
    // MODE bit0 = 1 -> override resistor preset, PFM bit1 = 1 -> FPWM (per datasheet).
    match dev.read_reg(addr::MODE) {
        Ok(raw) => {
            let mut mode = ModeBits::from_bits_truncate(raw);
            mode.insert(ModeBits::MODE);
            mode.insert(ModeBits::PFM);
            if let Err(e) = dev.write_reg(addr::MODE, mode.bits()) {
                println!("set FPWM failed: {:?}", e);
            }
        }
        Err(e) => println!("read MODE failed (cannot force PWM): {:?}", e),
    }

    // Finally enable output after all configuration is complete.
    if let Err(e) = dev.enable_output() {
        println!("enable_output failed: {:?}", e);
    }
}

pub fn log_status(mv: u16, mode: OperatingStatus, faults: FaultStatus) {
    if faults.short_circuit || faults.over_current || faults.over_voltage {
        println!(
            "WARN vset={}mV mode={:?} sc:{} oc:{} ov:{}",
            mv, mode, faults.short_circuit, faults.over_current, faults.over_voltage
        );
    } else {
        println!(
            "vset={}mV mode={:?} sc:{} oc:{} ov:{}",
            mv, mode, faults.short_circuit, faults.over_current, faults.over_voltage
        );
    }
}

pub fn log_mode_register(mode: ModeBits) {
    let oe = mode.contains(ModeBits::OE);
    let fsw2x = mode.contains(ModeBits::FSWDBL);
    let hiccup = mode.contains(ModeBits::HICCUP);
    let dischg = mode.contains(ModeBits::DISCHG);
    let vcc_ext = mode.contains(ModeBits::VCC_EXT);
    let i2c_alt = mode.contains(ModeBits::I2CADD);
    let override_from_reg = mode.contains(ModeBits::MODE);
    let pfm_bit = mode.contains(ModeBits::PFM);

    // Per datasheet: PFM bit=0 => PFM, PFM bit=1 => FPWM.
    let light_load = if override_from_reg {
        if pfm_bit {
            "forced FPWM"
        } else {
            "forced PFM"
        }
    } else if pfm_bit {
        "FPWM (from preset)"
    } else {
        "PFM (from preset)"
    };

    let vcc = if vcc_ext {
        "VCC=external 5V"
    } else {
        "VCC=internal LDO"
    };

    let addr = if i2c_alt {
        "I2C addr=0x75"
    } else {
        "I2C addr=0x74"
    };

    println!(
        "MODE=0x{:02X} oe:{} fsw:{} hiccup:{} dischg:{} {} {} light_load={}",
        mode.bits(),
        oe,
        if fsw2x { "2x" } else { "1x" },
        hiccup,
        dischg,
        vcc,
        addr,
        light_load
    );
}

pub fn log_status_and_mode<I2C>(dev: &mut Tps55288<I2C>, mv: u16)
where
    I2C: embedded_hal::i2c::I2c,
{
    if let Ok((mode, faults)) = dev.read_status() {
        log_status(mv, mode, faults);
    }
    if let Ok(raw_mode) = dev.read_reg(addr::MODE) {
        let mode = ModeBits::from_bits_truncate(raw_mode);
        log_mode_register(mode);
    } else {
        println!("read MODE failed in status loop");
    }
}

pub fn heartbeat(delay: &mut Delay) {
    delay.delay_millis(100);
}
