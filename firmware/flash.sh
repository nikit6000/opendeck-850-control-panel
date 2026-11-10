#!/bin/sh
# Flash the firmware.
#   ./flash.sh        — ST-Link via probe-rs (brew install probe-rs-tools). With an ST-Link V2 clone on
#                       macOS st-flash/openocd hung (libusb); probe-rs works.
#   ./flash.sh dfu    — USB ROM bootloader (hold BOOT0, tap NRST, release). Needs: brew install dfu-util
# Chip: STM32F401CC (CE works too: the image is < 256K). Override with CHIP=STM32F401CEUx.
set -e
cd "$(dirname "$0")"
./build.sh
ELF=target/thumbv7em-none-eabihf/release/cdj850-panel-adapter
if [ "$1" = dfu ]; then
  dfu-util -a 0 -d 0483:df11 -s 0x08000000:leave -D adapter.bin
else
  probe-rs download --chip ${CHIP:-STM32F401CCUx} --protocol swd "$ELF"
  probe-rs reset --chip ${CHIP:-STM32F401CCUx} --protocol swd
fi
