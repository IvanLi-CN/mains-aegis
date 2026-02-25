SHELL := /bin/bash

CARGO ?= cargo
PROBE_RS ?= probe-rs

ROOT_DIR := $(abspath $(CURDIR)/../..)
TARGET_TRIPLE := thumbv6m-none-eabi
CHIP := STM32G031G8
BIN_FIXED := fixed_5v
BIN_STEP := step_vout
BIN_EXT_FB := ext_fb_sw2303
ELF_FIXED := target/$(TARGET_TRIPLE)/release/$(BIN_FIXED)
ELF_STEP := target/$(TARGET_TRIPLE)/release/$(BIN_STEP)
ELF_EXT_FB := target/$(TARGET_TRIPLE)/release/$(BIN_EXT_FB)
SELECT_PROBE_SCRIPT := $(ROOT_DIR)/scripts/select-probe.sh

.PHONY: help select-probe build-fixed build-step build-ext-fb build \
        run-fixed run-step run-ext-fb \
        attach-fixed attach-step attach-ext-fb \
        reset require-probe

help:
	@echo 'STM32G031G8U6 TPS55288 examples:'
	@echo '  help                 - Show this message'
	@echo '  select-probe         - Interactively choose a debug probe (prints export command)'
	@echo
	@echo 'Build targets (release, thumbv6m-none-eabi):'
	@echo '  build-fixed          - Build bin=$(BIN_FIXED)'
	@echo '  build-step           - Build bin=$(BIN_STEP)'
	@echo '  build-ext-fb         - Build bin=$(BIN_EXT_FB)'
	@echo '  build                - Build all three binaries'
	@echo
	@echo 'Run (flash + reset via probe-rs):'
	@echo '  run-fixed            - probe-rs run $(BIN_FIXED)'
	@echo '  run-step             - probe-rs run $(BIN_STEP)'
	@echo '  run-ext-fb           - probe-rs run $(BIN_EXT_FB)'
	@echo
	@echo 'Attach / reset:'
	@echo '  attach-fixed         - Attach to $(BIN_FIXED)'
	@echo '  attach-step          - Attach to $(BIN_STEP)'
	@echo '  attach-ext-fb        - Attach to $(BIN_EXT_FB)'
	@echo '  reset                - Hardware reset only'
	@echo
	@echo 'Before run/attach/reset targets, set PROBE_ID:'
	@echo '  eval "$$(make -s select-probe)"'

select-probe:
	@$(SELECT_PROBE_SCRIPT)

require-probe:
	@if [ -z "$(PROBE_ID)" ]; then \
		echo 'PROBE_ID is not set. Run: eval "$$(make -s select-probe)"' >&2; \
		exit 1; \
	fi

build-fixed:
	$(CARGO) build --release --features hw --bin $(BIN_FIXED)

build-step:
	$(CARGO) build --release --features hw --bin $(BIN_STEP)

build-ext-fb:
	$(CARGO) build --release --features hw --bin $(BIN_EXT_FB)

build: build-fixed build-step build-ext-fb

run-fixed: require-probe build-fixed
	$(PROBE_RS) run --probe $(PROBE_ID) --chip $(CHIP) $(ELF_FIXED)

run-step: require-probe build-step
	$(PROBE_RS) run --probe $(PROBE_ID) --chip $(CHIP) $(ELF_STEP)

run-ext-fb: require-probe build-ext-fb
	$(PROBE_RS) run --probe $(PROBE_ID) --chip $(CHIP) $(ELF_EXT_FB)

attach-fixed: require-probe build-fixed
	$(PROBE_RS) attach --probe $(PROBE_ID) --chip $(CHIP) $(ELF_FIXED)

attach-step: require-probe build-step
	$(PROBE_RS) attach --probe $(PROBE_ID) --chip $(CHIP) $(ELF_STEP)

attach-ext-fb: require-probe build-ext-fb
	$(PROBE_RS) attach --probe $(PROBE_ID) --chip $(CHIP) $(ELF_EXT_FB)

reset: require-probe
	$(PROBE_RS) reset --probe $(PROBE_ID) --chip $(CHIP)
