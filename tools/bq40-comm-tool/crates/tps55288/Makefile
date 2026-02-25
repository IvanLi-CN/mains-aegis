## Default goal
.DEFAULT_GOAL := help

SHELL := /bin/bash

ROOT_DIR := $(abspath $(CURDIR))
EX_DIR := examples
SELECT_PROBE_SCRIPT := $(ROOT_DIR)/scripts/select-probe.sh

EXAMPLE_STM32G031 := stm32g031g8u6
EXAMPLE_ESP32S3FH4N2 := esp32s3fh4n2

.PHONY: help select-probe \
        run-stm32g031-fixed run-stm32g031-step run-stm32g031-ext-fb \
        build-stm32g031-fixed build-stm32g031-step build-stm32g031-ext-fb \
        attach-stm32g031-fixed attach-stm32g031-step attach-stm32g031-ext-fb \
        reset-stm32g031 \
        select-port \
        select-esp32s3fh4n2-port \
        build-esp32s3fh4n2-fixed build-esp32s3fh4n2-step build-esp32s3fh4n2-ext-fb build-esp32s3fh4n2 \
        run-esp32s3fh4n2-fixed run-esp32s3fh4n2-step run-esp32s3fh4n2-ext-fb \
        flash-esp32s3fh4n2-fixed flash-esp32s3fh4n2-step flash-esp32s3fh4n2-ext-fb \
        monitor-esp32s3fh4n2 monitor-esp32s3fh4n2-fixed monitor-esp32s3fh4n2-step monitor-esp32s3fh4n2-ext-fb \
        reset-esp32s3fh4n2

help:
	@echo 'Usage: make <target>'
	@echo
	@echo 'STM32G031G8U6 TPS55288 example targets (delegated to examples/$(EXAMPLE_STM32G031)/Makefile):'
	@echo '  (before run/attach/reset) eval "$$(make -s select-probe)"'
	@echo '  build-stm32g031-fixed      - Build fixed_5v (internal FB)'
	@echo '  build-stm32g031-step       - Build step_vout (internal FB)'
	@echo '  build-stm32g031-ext-fb     - Build ext_fb_sw2303 (external FB + SW2303)'
	@echo '  run-stm32g031-fixed        - Flash + run fixed_5v'
	@echo '  run-stm32g031-step         - Flash + run step_vout'
	@echo '  run-stm32g031-ext-fb       - Flash + run ext_fb_sw2303'
	@echo '  attach-stm32g031-fixed     - Attach to fixed_5v'
	@echo '  attach-stm32g031-step      - Attach to step_vout'
	@echo '  attach-stm32g031-ext-fb    - Attach to ext_fb_sw2303'
	@echo '  reset-stm32g031            - Reset target MCU'
	@echo
	@echo 'ESP32S3FH4N2 TPS55288 example targets (delegated to examples/$(EXAMPLE_ESP32S3FH4N2)/Makefile):'
	@echo '  build-esp32s3fh4n2-fixed   - Build fixed_5v (internal FB)'
	@echo '  build-esp32s3fh4n2-step    - Build step_vout (internal FB)'
	@echo '  build-esp32s3fh4n2-ext-fb  - Build ext_fb_sw2303 (external FB + SW2303)'
	@echo '  build-esp32s3fh4n2         - Build all three binaries'
	@echo '  run-esp32s3fh4n2-fixed     - Flash + run fixed_5v (via cargo runner)'
	@echo '  run-esp32s3fh4n2-step      - Flash + run step_vout (via cargo runner)'
	@echo '  run-esp32s3fh4n2-ext-fb    - Flash + run ext_fb_sw2303 (via cargo runner)'
	@echo '  (for flash/monitor/reset)  eval "$$(make -s select-port)"'
	@echo '  flash-esp32s3fh4n2-fixed   - espflash flash --monitor fixed_5v'
	@echo '  flash-esp32s3fh4n2-step    - espflash flash --monitor step_vout'
	@echo '  flash-esp32s3fh4n2-ext-fb  - espflash flash --monitor ext_fb_sw2303'
	@echo '  monitor-esp32s3fh4n2       - espflash monitor (no flash)'
	@echo '  reset-esp32s3fh4n2         - espflash reset'
	@echo
	@echo 'Examples:'
	@echo '  make run-stm32g031-ext-fb'
	@echo '  make build-stm32g031-fixed'
	@echo '  make run-esp32s3fh4n2-fixed'
	@echo '  eval "$$(make -s select-port)" && make flash-esp32s3fh4n2-fixed'

select-probe:
	@$(SELECT_PROBE_SCRIPT)

select-port: select-esp32s3fh4n2-port

select-esp32s3fh4n2-port:
	@$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) select-port

build-stm32g031-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) build-fixed

build-stm32g031-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) build-step

build-stm32g031-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) build-ext-fb

run-stm32g031-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) run-fixed

run-stm32g031-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) run-step

run-stm32g031-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) run-ext-fb

attach-stm32g031-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) attach-fixed

attach-stm32g031-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) attach-step

attach-stm32g031-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) attach-ext-fb

reset-stm32g031:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_STM32G031) reset

build-esp32s3fh4n2-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) build-fixed

build-esp32s3fh4n2-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) build-step

build-esp32s3fh4n2-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) build-ext-fb

build-esp32s3fh4n2:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) build

run-esp32s3fh4n2-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) run-fixed

run-esp32s3fh4n2-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) run-step

run-esp32s3fh4n2-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) run-ext-fb

flash-esp32s3fh4n2-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) flash-fixed

flash-esp32s3fh4n2-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) flash-step

flash-esp32s3fh4n2-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) flash-ext-fb

monitor-esp32s3fh4n2:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) monitor

monitor-esp32s3fh4n2-fixed:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) monitor-fixed

monitor-esp32s3fh4n2-step:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) monitor-step

monitor-esp32s3fh4n2-ext-fb:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) monitor-ext-fb

reset-esp32s3fh4n2:
	$(MAKE) -C $(EX_DIR)/$(EXAMPLE_ESP32S3FH4N2) reset
