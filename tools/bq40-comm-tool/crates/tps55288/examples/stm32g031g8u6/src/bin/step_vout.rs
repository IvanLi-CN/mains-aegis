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
    info!("TPS55288 step-vout demo (PB6/PB7 I2C1, PB5 EN)");
    let mut board = init_board();
    board.en.set_high();
    let mut dev = Tps55288::new(board.i2c);
    setup_device(&mut dev).await;

    let mut mv: u16 = 3_300;
    loop {
        let _ = dev.set_vout_mv(mv).await;
        log_status_and_mode(&mut dev, mv).await;
        heartbeat(&mut board.led).await;
        mv = if mv + 20 <= 21_000 { mv + 20 } else { 3_300 };
    }
}
