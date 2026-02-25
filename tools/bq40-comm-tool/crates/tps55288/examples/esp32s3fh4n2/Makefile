SHELL := /bin/bash

CARGO ?= cargo
ESPFLASH ?= espflash

CHIP ?= esp32s3
TARGET_TRIPLE ?= xtensa-esp32s3-none-elf

BIN_FIXED := fixed_5v
BIN_STEP := step_vout
BIN_EXT_FB := ext_fb_sw2303

ELF_FIXED := target/$(TARGET_TRIPLE)/release/$(BIN_FIXED)
ELF_STEP := target/$(TARGET_TRIPLE)/release/$(BIN_STEP)
ELF_EXT_FB := target/$(TARGET_TRIPLE)/release/$(BIN_EXT_FB)

# You can set either PORT=... or ESPFLASH_PORT=...
PORT ?= $(ESPFLASH_PORT)

.PHONY: help select-port require-port \
        build-fixed build-step build-ext-fb build \
        run-fixed run-step run-ext-fb \
        flash-fixed flash-step flash-ext-fb \
        monitor monitor-fixed monitor-step monitor-ext-fb \
        reset clean

help:
	@echo 'ESP32S3FH4N2 TPS55288 examples:'
	@echo '  help                 - Show this message'
	@echo '  select-port          - Interactively choose a serial port (prints export command)'
	@echo
	@echo 'Build targets (release, $(TARGET_TRIPLE))'
	@echo '  build-fixed          - Build bin=$(BIN_FIXED)'
	@echo '  build-step           - Build bin=$(BIN_STEP)'
	@echo '  build-ext-fb         - Build bin=$(BIN_EXT_FB)'
	@echo '  build                - Build all binaries'
	@echo
	@echo 'Run via Cargo runner (uses .cargo/config.toml runner = espflash flash --monitor):'
	@echo '  run-fixed            - cargo +esp run --release --bin $(BIN_FIXED)'
	@echo '  run-step             - cargo +esp run --release --bin $(BIN_STEP)'
	@echo '  run-ext-fb           - cargo +esp run --release --bin $(BIN_EXT_FB)'
	@echo
	@echo 'Direct espflash (requires PORT/ESPFLASH_PORT):'
	@echo '  flash-fixed          - espflash flash $(BIN_FIXED)'
	@echo '  flash-step           - espflash flash $(BIN_STEP)'
	@echo '  flash-ext-fb         - espflash flash $(BIN_EXT_FB)'
	@echo '  monitor              - espflash monitor (no flash)'
	@echo '  reset                - espflash reset'
	@echo
	@echo 'Before flash/monitor/reset, set ESPFLASH_PORT (or PORT):'
	@echo '  eval "$$(make -s select-port)"'

select-port:
	@if ! command -v $(ESPFLASH) >/dev/null 2>&1; then \
		echo "$(ESPFLASH) command not found. Install espflash first." >&2; \
		exit 1; \
	fi
	@ports=($$($(ESPFLASH) list-ports --name-only --skip-update-check)); \
	if [[ $${#ports[@]} -eq 0 ]]; then \
		echo "No serial ports detected. Connect an ESP32-S3 board and retry." >&2; \
		exit 1; \
	fi; \
	echo "Detected serial ports:" >&2; \
	for i in "$${!ports[@]}"; do \
		printf '  [%d] %s\n' $$((i + 1)) "$${ports[$$i]}" >&2; \
	done; \
	echo >&2; \
	read -r -p "Select port index or enter path: " choice; \
	if [[ -z "$$choice" ]]; then \
		echo "Selection cancelled." >&2; \
		exit 1; \
	fi; \
	selected=""; \
	if [[ "$$choice" =~ ^[0-9]+$$ ]]; then \
		idx=$$((choice - 1)); \
		if [[ $$idx -lt 0 || $$idx -ge $${#ports[@]} ]]; then \
			echo "Invalid port index: $$choice" >&2; \
			exit 1; \
		fi; \
		selected="$${ports[$$idx]}"; \
	else \
		selected="$$choice"; \
	fi; \
	echo "export ESPFLASH_PORT=$${selected}"

require-port:
	@if [[ -z "$(PORT)" ]]; then \
		echo 'ESPFLASH_PORT (or PORT) is not set. Run: eval "$$(make -s select-port)"' >&2; \
		exit 1; \
	fi

build-fixed:
	$(CARGO) +esp build --release --bin $(BIN_FIXED)

build-step:
	$(CARGO) +esp build --release --bin $(BIN_STEP)

build-ext-fb:
	$(CARGO) +esp build --release --bin $(BIN_EXT_FB)

build: build-fixed build-step build-ext-fb

run-fixed:
	$(CARGO) +esp run --release --bin $(BIN_FIXED)

run-step:
	$(CARGO) +esp run --release --bin $(BIN_STEP)

run-ext-fb:
	$(CARGO) +esp run --release --bin $(BIN_EXT_FB)

flash-fixed: require-port build-fixed
	$(ESPFLASH) flash --skip-update-check --chip $(CHIP) --port $(PORT) --monitor $(ELF_FIXED)

flash-step: require-port build-step
	$(ESPFLASH) flash --skip-update-check --chip $(CHIP) --port $(PORT) --monitor $(ELF_STEP)

flash-ext-fb: require-port build-ext-fb
	$(ESPFLASH) flash --skip-update-check --chip $(CHIP) --port $(PORT) --monitor $(ELF_EXT_FB)

monitor: require-port
	$(ESPFLASH) monitor --skip-update-check --chip $(CHIP) --port $(PORT)

monitor-fixed: require-port build-fixed
	$(ESPFLASH) monitor --skip-update-check --chip $(CHIP) --port $(PORT) --elf $(ELF_FIXED)

monitor-step: require-port build-step
	$(ESPFLASH) monitor --skip-update-check --chip $(CHIP) --port $(PORT) --elf $(ELF_STEP)

monitor-ext-fb: require-port build-ext-fb
	$(ESPFLASH) monitor --skip-update-check --chip $(CHIP) --port $(PORT) --elf $(ELF_EXT_FB)

reset: require-port
	$(ESPFLASH) reset --skip-update-check --chip $(CHIP) --port $(PORT)

clean:
	rm -rf target
