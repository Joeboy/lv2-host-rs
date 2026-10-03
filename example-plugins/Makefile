ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD_DIR := $(ROOT)/build

ABGATE_REV := bcb61514760ead02511620979749fb55a8b447bd
LIBREARP_REV := 05547ac465775cab983e00386e259793eb4fc50b
FURELISE_REV := e4f87851b0504b846e19e4cf8a0c7fbfcfd933c8
AMS_REV := c219c426d8b32b5b347f1ee74fc5517447088fd0

.PHONY: all fetch build install test

all: install

fetch:
	@if [ ! -e "$(ROOT)/abGate/.git" ]; then git clone https://github.com/antanasbruzas/abGate "$(ROOT)/abGate"; fi
	@git -C "$(ROOT)/abGate" fetch origin $(ABGATE_REV)
	@git -C "$(ROOT)/abGate" checkout --detach $(ABGATE_REV)
	@if [ ! -e "$(ROOT)/LibreArp/.git" ]; then git clone https://github.com/LibreArp/LibreArp "$(ROOT)/LibreArp"; fi
	@git -C "$(ROOT)/LibreArp" fetch origin 2.5-lv2
	@git -C "$(ROOT)/LibreArp" checkout --detach $(LIBREARP_REV)
	@git -C "$(ROOT)/LibreArp" submodule update --init --recursive
	@if [ ! -e "$(ROOT)/furelise-lv2/.git" ]; then git clone https://github.com/Joeboy/furelise-lv2 "$(ROOT)/furelise-lv2"; fi
	@git -C "$(ROOT)/furelise-lv2" fetch origin $(FURELISE_REV)
	@git -C "$(ROOT)/furelise-lv2" checkout --detach $(FURELISE_REV)
	@if [ ! -e "$(ROOT)/ams-lv2/.git" ]; then git clone https://github.com/Joeboy/ams-lv2 "$(ROOT)/ams-lv2"; fi
	@git -C "$(ROOT)/ams-lv2" fetch origin $(AMS_REV)
	@git -C "$(ROOT)/ams-lv2" checkout --detach $(AMS_REV)

build: fetch
	+$(MAKE) -C "$(ROOT)/abGate" all
	cmake -S "$(ROOT)/LibreArp" -B "$(ROOT)/LibreArp/build" -DCMAKE_BUILD_TYPE=Release
	cmake --build "$(ROOT)/LibreArp/build" --target LibreArp_LV2
	+$(MAKE) -C "$(ROOT)/furelise-lv2" bundle
	cd "$(ROOT)/ams-lv2" && ./waf configure
	cd "$(ROOT)/ams-lv2" && ./waf build
	mkdir -p "$(BUILD_DIR)/gtk3-gain.lv2"
	$(CC) -std=c11 -fPIC -shared -o "$(BUILD_DIR)/gtk3-gain.lv2/gain.so" "$(ROOT)/gtk3-gain/gain.c"
	$(CC) -std=c11 -fPIC -shared $$(pkg-config --cflags gtk+-3.0) -o "$(BUILD_DIR)/gtk3-gain.lv2/gain-ui.so" "$(ROOT)/gtk3-gain/gain-ui.c" $$(pkg-config --libs gtk+-3.0)

install: build
	mkdir -p "$(BUILD_DIR)"
	cp -a "$(ROOT)/abGate/abGate.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/LibreArp/build/LibreArp_artefacts/Release/LV2/LibreArp.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/furelise-lv2/build/furelise.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/ams-lv2/build/ams.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/gtk3-gain/manifest.ttl" "$(ROOT)/gtk3-gain/gain.ttl" "$(BUILD_DIR)/gtk3-gain.lv2/"

test: install
	LV2_PATH="$(BUILD_DIR)" xvfb-run -a cargo test --manifest-path "$(ROOT)/../Cargo.toml" -- --test-threads=1
