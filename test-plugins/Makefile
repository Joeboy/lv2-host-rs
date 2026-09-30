ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD_DIR := $(ROOT)/build

ABGATE_REV := bcb61514760ead02511620979749fb55a8b447bd
LIBREARP_REV := 05547ac465775cab983e00386e259793eb4fc50b
FURELISE_REV := e4f87851b0504b846e19e4cf8a0c7fbfcfd933c8

.PHONY: all fetch build install

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

build: fetch
	+$(MAKE) -C "$(ROOT)/abGate" all
	cmake -S "$(ROOT)/LibreArp" -B "$(ROOT)/LibreArp/build" -DCMAKE_BUILD_TYPE=Release
	cmake --build "$(ROOT)/LibreArp/build" --target LibreArp_LV2
	+$(MAKE) -C "$(ROOT)/furelise-lv2" bundle

install: build
	mkdir -p "$(BUILD_DIR)"
	cp -a "$(ROOT)/abGate/abGate.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/LibreArp/build/LibreArp_artefacts/Release/LV2/LibreArp.lv2" "$(BUILD_DIR)/"
	cp -a "$(ROOT)/furelise-lv2/build/furelise.lv2" "$(BUILD_DIR)/"
