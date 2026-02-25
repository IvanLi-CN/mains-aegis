use defmt::{info, warn};
use embassy_stm32::{
    gpio::{Level, Output, Speed},
    i2c::{Config as I2cConfig, I2c},
    mode::Async,
    time::Hertz,
};
use embassy_time::{Duration, Timer};

use tps55288::data_types::{
    CableCompLevel, CableCompOption, FaultStatus, FeedbackSource, InternalFeedbackRatio, OcpDelay,
    OperatingStatus, VoutSlewRate,
};
use tps55288::driver::Tps55288;
use tps55288::registers::{addr, ModeBits};

/// Concrete I2C type for I2C1 on PB6/PB7 using DMA1 channels (async mode).
pub type BoardI2c = I2c<'static, Async>;

pub struct Board {
    pub i2c: BoardI2c,
    pub en: Output<'static>,
    pub led: Output<'static>,
}

/// Initialize peripherals following the sc8815-rs STM32G0 pattern.
pub fn init_board() -> Board {
    let p = embassy_stm32::init(Default::default());

    let mut i2c_cfg = I2cConfig::default();
    i2c_cfg.scl_pullup = true;
    i2c_cfg.sda_pullup = true;

    let i2c = I2c::new(
        p.I2C1,
        p.PB6, // SCL
        p.PB7, // SDA
        super::Irqs,
        p.DMA1_CH1,
        p.DMA1_CH2,
        Hertz(100_000),
        i2c_cfg,
    );

    // EN is wired to PB5 on current hardware; start high to allow I2C access.
    let en = Output::new(p.PB5, Level::High, Speed::Low);
    // PB8 is available for a heartbeat LED.
    let led = Output::new(p.PB8, Level::High, Speed::Low);

    Board { i2c, en, led }
}

/// Apply a safe baseline configuration using internal DAC feedback (FB pin unused).
pub async fn setup_device<I2C>(dev: &mut Tps55288<I2C>)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    info!("Configuring TPS55288 with internal DAC feedback (OE disabled)");

    if let Err(e) = dev.init().await {
        warn!("init failed: {:?}", defmt::Debug2Format(&e));
    }
    if let Err(e) = dev.set_ilim_ma(3_000, true).await {
        warn!("set_ilim failed: {:?}", defmt::Debug2Format(&e));
    }
    // Use the smallest internal divider (R0_0564) so the REF DAC maps 0.8–21 V correctly.
    if let Err(e) = dev
        .set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564)
        .await
    {
        warn!("set_feedback failed: {:?}", defmt::Debug2Format(&e));
    }
    if let Err(e) = dev
        .set_cable_comp(
            CableCompOption::Internal,
            CableCompLevel::V0p0,
            true,
            true,
            true,
        )
        .await
    {
        warn!("set_cable_comp failed: {:?}", defmt::Debug2Format(&e));
    }
    if let Err(e) = dev
        .set_vout_sr(VoutSlewRate::Sr2p5MvPerUs, OcpDelay::Us128)
        .await
    {
        warn!("set_vout_sr failed: {:?}", defmt::Debug2Format(&e));
    }

    // Force FPWM at light load using MODE register:
    // MODE bit0 = 1 -> override resistor preset, PFM bit1 = 1 -> FPWM (per datasheet).
    if let Ok(raw) = dev.read_reg(addr::MODE).await {
        let mut mode = ModeBits::from_bits_truncate(raw);
        mode.insert(ModeBits::MODE);
        mode.insert(ModeBits::PFM);
        if let Err(e) = dev.write_reg(addr::MODE, mode.bits()).await {
            warn!("set FPWM failed: {:?}", defmt::Debug2Format(&e));
        }
    } else {
        warn!("read MODE failed (cannot force PWM)");
    }

    // Finally enable output after all configuration is complete.
    if let Err(e) = dev.enable_output().await {
        warn!("enable_output failed: {:?}", defmt::Debug2Format(&e));
    }
}

pub fn log_status(mv: u16, mode: OperatingStatus, faults: FaultStatus) {
    if faults.short_circuit || faults.over_current || faults.over_voltage {
        warn!(
            "vset={}mV mode={:?} sc:{} oc:{} ov:{}",
            mv, mode, faults.short_circuit, faults.over_current, faults.over_voltage
        );
    } else {
        info!(
            "vset={}mV mode={:?} sc:{} oc:{} ov:{}",
            mv, mode, faults.short_circuit, faults.over_current, faults.over_voltage
        );
    }
}

/// Decode MODE register into a human-readable summary.
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

    info!(
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

/// Log STATUS and MODE registers in one shot (async I2C).
pub async fn log_status_and_mode<I2C>(dev: &mut Tps55288<I2C>, mv: u16)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    if let Ok((mode, faults)) = dev.read_status().await {
        log_status(mv, mode, faults);
    }
    if let Ok(raw_mode) = dev.read_reg(addr::MODE).await {
        let mode = ModeBits::from_bits_truncate(raw_mode);
        log_mode_register(mode);
    } else {
        warn!("read MODE failed in status loop");
    }
}

pub async fn heartbeat(led: &mut Output<'_>) {
    led.toggle();
    // 100 ms heartbeat / log period for higher-frequency status sampling.
    Timer::after(Duration::from_millis(100)).await;
}
