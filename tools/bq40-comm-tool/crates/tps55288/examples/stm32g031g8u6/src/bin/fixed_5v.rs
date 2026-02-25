#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{bind_interrupts, i2c};
use {defmt_rtt as _, panic_probe as _};

mod common;
use common::{heartbeat, init_board, log_status_and_mode, setup_device};
use tps55288::driver::Tps55288;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::EventInterruptHandler<embassy_stm32::peripherals::I2C1>, i2c::ErrorInterruptHandler<embassy_stm32::peripherals::I2C1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("TPS55288 fixed 5V demo (PB6/PB7 I2C1, PB5 EN)");
    let mut board = init_board();
    board.en.set_high();
    let mut dev = Tps55288::new(board.i2c);
    setup_device(&mut dev).await;

    // Default demo: fixed 5 V output using internal DAC feedback.
    let target = 5_000u16;
    let _ = dev.set_vout_mv(target).await;

    loop {
        log_status_and_mode(&mut dev, target).await;
        heartbeat(&mut board.led).await;
    }
}
