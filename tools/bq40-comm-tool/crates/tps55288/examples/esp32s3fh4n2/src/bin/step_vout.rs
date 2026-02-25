#![no_std]
#![no_main]

use esp_hal::main;
use esp_println::println;

use tps55288::driver::Tps55288;

use esp_backtrace as _;

mod common;
use common::{heartbeat, init_board, log_status_and_mode, setup_device};

#[main]
fn main() -> ! {
    println!("TPS55288 step-vout demo (SCL=GPIO40 pin45, SDA=GPIO39 pin44, INT=GPIO38 pin43, CE~=GPIO37 pin42)");

    let mut board = init_board();

    // CE is hardware-inverted on this board: LOW enables TPS55288.
    board.ce_inverted.set_low();

    let mut dev = Tps55288::new(board.i2c);
    setup_device(&mut dev);

    let mut mv: u16 = 3_300;
    loop {
        let _ = dev.set_vout_mv(mv);
        log_status_and_mode(&mut dev, mv);
        heartbeat(&mut board.delay);
        mv = if mv + 20 <= 21_000 { mv + 20 } else { 3_300 };
    }
}
