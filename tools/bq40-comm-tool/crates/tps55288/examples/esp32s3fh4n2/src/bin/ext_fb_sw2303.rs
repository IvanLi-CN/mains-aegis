#![no_std]
#![no_main]

use esp_hal::main;
use esp_println::println;

use tps55288::data_types::{
    CableCompLevel, CableCompOption, FeedbackSource, InternalFeedbackRatio, OcpDelay, VoutSlewRate,
};
use tps55288::driver::Tps55288;
use tps55288::registers::{addr, ModeBits};

use esp_backtrace as _;

mod common;
use common::{heartbeat, init_board, log_status_and_mode};

#[main]
fn main() -> ! {
    println!("TPS55288 external-FB + SW2303 demo (SCL=GPIO40 pin45, SDA=GPIO39 pin44, INT=GPIO38 pin43, CE~=GPIO37 pin42)");

    let mut board = init_board();

    // CE is hardware-inverted on this board: LOW enables TPS55288.
    board.ce_inverted.set_low();

    let mut dev = Tps55288::new(board.i2c);

    // Configure current limit first so SW2303 never sees an unlimited power stage.
    if let Err(e) = dev.set_ilim_ma(3_000, true) {
        println!("set_ilim failed: {:?}", e);
    }

    // Switch to *external* feedback network on FB/INT (SW2303 + resistor divider).
    // INTFB ratio bits are ignored in external mode; keep the reset value (0.0564).
    if let Err(e) = dev.set_feedback(FeedbackSource::External, InternalFeedbackRatio::R0_0564) {
        println!("set_feedback failed: {:?}", e);
    }

    // Keep internal cable compensation disabled (0 V droop compensation) and mask bits enabled.
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

    // Program REF DAC for ~1.2 V at FB/INT in external feedback mode.
    // Datasheet (REFERENCE VOLTAGE table) shows REF=03FFh -> VREF ≈ 1.2 V.
    // With Rtop=100 kΩ, Rbottom=31.6 kΩ:
    //   VOUT ≈ VREF * (1 + Rtop/Rbottom) ≈ 1.2 V * 4.1646 ≈ 5.0 V
    // This makes the default external-FB output ≈5 V.
    let ref_code: u16 = 0x03FF; // 10-bit full-scale
    let ref_bytes = ref_code.to_le_bytes();
    if let Err(e) = dev.write_regs(addr::REF0, &ref_bytes) {
        println!("set REF (1.2V) failed: {:?}", e);
    }

    // Finally, enable the output (OE bit in MODE register) and force FPWM at light load.
    // In external FB mode, SW2303 + the resistor network define VOUT for a given REF code.
    if let Ok(raw) = dev.read_reg(addr::MODE) {
        let mut mode = ModeBits::from_bits_truncate(raw);
        mode.insert(ModeBits::MODE);
        mode.insert(ModeBits::PFM);
        mode.insert(ModeBits::OE);
        if let Err(e) = dev.write_reg(addr::MODE, mode.bits()) {
            println!("enable OE failed: {:?}", e);
        }
    } else {
        println!("read MODE failed (cannot enable OE cleanly)");
    }

    // From this point on we deliberately *do not* call set_vout_mv:
    // - TPS55288 is used purely as a power stage with external FB.
    // - SW2303 + the FB network own the actual output voltage selection.
    let requested_mv: u16 = 5_000;
    loop {
        log_status_and_mode(&mut dev, requested_mv);
        heartbeat(&mut board.delay);
    }
}
