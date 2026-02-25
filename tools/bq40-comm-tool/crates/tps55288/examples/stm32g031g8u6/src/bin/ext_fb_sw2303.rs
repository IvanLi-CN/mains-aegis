#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{bind_interrupts, i2c};
use embassy_time::{Duration, Timer};
use {defmt_rtt as _, panic_probe as _};

mod common;
use common::{init_board, log_status_and_mode};
use tps55288::data_types::{
    CableCompLevel, CableCompOption, FeedbackSource, InternalFeedbackRatio, OcpDelay, VoutSlewRate,
};
use tps55288::driver::Tps55288;
use tps55288::registers::{addr, ModeBits};

bind_interrupts!(struct Irqs {
    I2C1 => i2c::EventInterruptHandler<embassy_stm32::peripherals::I2C1>,
            i2c::ErrorInterruptHandler<embassy_stm32::peripherals::I2C1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("TPS55288 external-FB + SW2303 demo (PB6/PB7 I2C1, PB5 EN)");

    let mut board = init_board();
    board.en.set_high();

    let mut dev = Tps55288::new(board.i2c);

    // Configure current limit first so SW2303 never sees an unlimited power stage.
    if let Err(e) = dev.set_ilim_ma(3_000, true).await {
        defmt::warn!("set_ilim failed: {:?}", defmt::Debug2Format(&e));
    }

    // Switch to *external* feedback network on FB/INT (SW2303 + resistor divider).
    // INTFB ratio bits are ignored in external mode; keep the reset value (0.0564).
    if let Err(e) = dev
        .set_feedback(FeedbackSource::External, InternalFeedbackRatio::R0_0564)
        .await
    {
        defmt::warn!("set_feedback failed: {:?}", defmt::Debug2Format(&e));
    }

    // Keep internal cable compensation disabled (0 V droop compensation) and mask bits enabled.
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
        defmt::warn!("set_cable_comp failed: {:?}", defmt::Debug2Format(&e));
    }

    if let Err(e) = dev
        .set_vout_sr(VoutSlewRate::Sr2p5MvPerUs, OcpDelay::Us128)
        .await
    {
        defmt::warn!("set_vout_sr failed: {:?}", defmt::Debug2Format(&e));
    }

    // Program REF DAC for ~1.2 V at FB/INT in external feedback mode.
    // Datasheet (REFERENCE VOLTAGE table) shows REF=03FFh -> VREF ≈ 1.2 V.
    // With Rtop=100 kΩ, Rbottom=31.6 kΩ:
    //   VOUT ≈ VREF * (1 + Rtop/Rbottom) ≈ 1.2 V * 4.1646 ≈ 5.0 V
    // This makes the default external-FB output ≈5 V.
    let ref_code: u16 = 0x03FF; // 10-bit full-scale
    let ref_bytes = ref_code.to_le_bytes();
    if let Err(e) = dev.write_regs(addr::REF0, &ref_bytes).await {
        defmt::warn!("set REF (1.2V) failed: {:?}", defmt::Debug2Format(&e));
    }

    // Finally, enable the output (OE bit in MODE register) and force FPWM at light load.
    // In external FB mode, SW2303 + the resistor network define VOUT for a given REF code.
    if let Ok(raw) = dev.read_reg(addr::MODE).await {
        let mut mode = ModeBits::from_bits_truncate(raw);
        // Force FPWM using MODE register: MODE=1 -> override preset, PFM=1 -> FPWM (per datasheet).
        mode.insert(ModeBits::MODE);
        mode.insert(ModeBits::PFM);
        // Enable output while keeping other bits from the MODE pin preset.
        mode.insert(ModeBits::OE);
        if let Err(e) = dev.write_reg(addr::MODE, mode.bits()).await {
            defmt::warn!("enable OE failed: {:?}", defmt::Debug2Format(&e));
        }
    } else {
        defmt::warn!("read MODE failed (cannot enable OE cleanly)");
    }

    // From this point on we deliberately *do not* call set_vout_mv:
    // - TPS55288 is used purely as a power stage with external FB.
    // - SW2303 + the FB network own the actual output voltage selection.
    let requested_mv: u16 = 5_000;
    loop {
        log_status_and_mode(&mut dev, requested_mv).await;
        // 100 ms heartbeat + log period for higher-frequency status sampling.
        board.led.toggle();
        Timer::after(Duration::from_millis(100)).await;
    }
}
