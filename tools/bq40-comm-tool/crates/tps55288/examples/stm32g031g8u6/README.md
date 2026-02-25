# STM32G031G8U6 Examples

Rust/Embassy demos for driving TPS55288 from an STM32G031G8U6 board.

## Demos
- `fixed_5v`: Internal DAC feedback, start at 5 V and poll faults.
- `step_vout`: Internal DAC feedback, sweep VOUT between 3.3 V and ~21 V.
- `ext_fb_sw2303`: External FB network on FB/INT pin plus SW2303 PD controller; TPS55288 drives the REF DAC while SW2303 stays in its default profile.

## Hardware Assumptions
- STM32G031G8U6 board, I2C1 on PB6 (SCL) / PB7 (SDA) with pull-ups.
- EN pin tied to PB5 (push-pull). IRQ/INT (FB/INT pin) wired into the external FB network (SW2303 + resistor divider).
- VIN bench supply (9–20 V). VOUT to load or USB-C connector via SW2303 module.
- I2C pull-ups present.

## Build / Flash
From the workspace root:

```bash
cd examples/stm32g031g8u6
cargo build --release --features hw --bin fixed_5v
cargo build --release --features hw --bin step_vout
cargo build --release --features hw --bin ext_fb_sw2303
```

Use your usual `probe-rs` / OpenOCD workflow to flash the resulting binaries.
