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
    println!("TPS55288 fixed 12V demo (SCL=GPIO40 pin45, SDA=GPIO39 pin44, INT=GPIO38 pin43, CE~=GPIO37 pin42)");

    let mut board = init_board();

    // CE is hardware-inverted on this board: LOW enables TPS55288.
    board.ce_inverted.set_low();

    let mut dev = Tps55288::new(board.i2c);
    setup_device(&mut dev);

    let target = 12_000u16;
    let _ = dev.set_vout_mv(target);

    let mut last_int_low = board.int.is_low();
    loop {
        log_status_and_mode(&mut dev, target);

        let int_low = board.int.is_low();
        if int_low != last_int_low {
            println!(
                "INT changed: {}",
                if int_low { "LOW(asserted)" } else { "HIGH" }
            );
            last_int_low = int_low;
        }

        heartbeat(&mut board.delay);
    }
}
