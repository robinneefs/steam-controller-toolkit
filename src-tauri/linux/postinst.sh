#!/bin/sh
# Apply the udev rule now, so the controller and /dev/uinput work without a restart.
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
  udevadm trigger --subsystem-match=hidraw --subsystem-match=misc || true
fi
exit 0
