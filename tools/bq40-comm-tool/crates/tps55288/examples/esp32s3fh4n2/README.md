# ESP32S3FH4N2 Examples

Rust demos for driving TPS55288 from an ESP32-S3FH4N2.

## Demos
- `fixed_5v`: Internal DAC feedback, start at 5 V and poll faults.
- `step_vout`: Internal DAC feedback, sweep VOUT between 3.3 V and ~21 V.
- `ext_fb_sw2303`: External FB network on FB/INT pin plus SW2303 PD controller; TPS55288 drives the REF DAC while SW2303 stays in its default profile.

## Hardware Assumptions
- I2C: SCL on **MCU pin 45 = GPIO40 (MTDO)**, SDA on **MCU pin 44 = GPIO39 (MTCK)**.
- INT: **MCU pin 43 = GPIO38**.
- EN: **MCU pin 42 = GPIO37**, but this net is actually **CE** and is **hardware-inverted**.
  - In other words: **drive GPIO37 LOW to enable TPS55288**.
- I2C pull-ups present.

Notes:
- GPIO39/GPIO40 are also JTAG pins (MTCK/MTDO). If you rely on external JTAG, pick different GPIOs for I2C.

## Build / Flash
From the workspace root:

```bash
cd examples/esp32s3fh4n2

# This example targets Xtensa, so use the ESP Rust toolchain (installed by espup/esp-rs).
# The provided `.cargo/config.toml` enables `build-std=core` and the required linker scripts.
# This example also embeds the ESP-IDF app descriptor via `esp_bootloader_esp_idf::esp_app_desc!()`,
# which `espflash` expects by default.
cargo +esp build --release --bin fixed_5v
cargo +esp build --release --bin step_vout
cargo +esp build --release --bin ext_fb_sw2303

# If you have .cargo/config.toml runner set up, you can also:
# cargo +esp run --release --bin fixed_5v
```

## Makefile shortcuts
```bash
cd examples/esp32s3fh4n2
make build
make run-fixed

# or use explicit port:
eval "$(make -s select-port)"
make flash-fixed
```

To flash manually with `espflash`:

```bash
espflash flash --chip esp32s3 --monitor target/xtensa-esp32s3-none-elf/release/fixed_5v
```
