#!/bin/sh
# Build the firmware and make adapter.bin (for DFU / ST-Link).
set -e
cd "$(dirname "$0")"
# rustup from Homebrew is keg-only; needs: rustup target add thumbv7em-none-eabihf; rustup component add llvm-tools
[ -d /opt/homebrew/opt/rustup/bin ] && export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo build --release
OBJCOPY=$(ls ~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objcopy | head -1)
$OBJCOPY -O binary target/thumbv7em-none-eabihf/release/cdj850-panel-adapter adapter.bin
ls -l adapter.bin
