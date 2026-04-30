# work48 — M48: Sensors / I²C / SMBus / GPIO / hwmon (laptop sensors, fan control)

> Derived from `docs/idea.md` §7 (Device Driver Framework — SMBus / I²C / GPIO for laptop sensors).

## Goal

Add the small but indispensable sensor surface every laptop OS needs. Land **I²C/SMBus** controller drivers, a **GPIO** chardev (`/dev/gpiochip*`), an **hwmon** subsystem (`/sys/class/hwmon/*`) that surfaces temperature, voltage, fan, power, current, and humidity, and a userspace `willosensors`/`willofancontrol` pair shaped like `lm-sensors`/`fancontrol`.

## Depends on

- **M13** — PCI + interrupt routing (SMBus is a PCI device on most chipsets).
- **M23** — power/thermal (hwmon feeds the thermal_zone tree).
- **M45** — loadable modules (per-vendor sensor chips ship as modules).

## Acceptance criteria

- [ ] `/dev/i2c-N` char dev opens; `i2c_smbus_*` ops round-trip against a virtual sensor in QEMU.
- [ ] `/dev/gpiochip*` exposes lines with the chardev v2 ABI (`GPIO_V2_LINE_GET_VALUES_IOCTL`, etc.).
- [ ] hwmon class enumerates registered chips under `/sys/class/hwmon/hwmonN/{name,temp1_input,fan1_input,...}`.
- [ ] `willosensors` prints temp/voltage/fan/power table from hwmon.
- [ ] `willofancontrol /etc/willo/fancontrol.conf` ramps PWM based on temperature; ramp curves work.
- [ ] §36 system monitor surfaces sensor graphs (CPU package temp, fan RPM, battery current).
- [ ] `kernel/tests/i2c_smbus_basic.rs`, `kernel/tests/gpio_chardev.rs`, `kernel/tests/hwmon_register.rs` pass.

## Task breakdown

### T1. I²C core — `kernel/src/i2c/core.rs`
- `I2cBus`, `I2cDriver`, `I2cClient` traits.
- `i2c_transfer(msgs)`, SMBus convenience: `read_byte`, `read_word_data`, `block_read`.

### T2. SMBus host driver — `kernel/src/i2c/host/i801.rs`
- Intel ICH/PCH SMBus (i801-class); polled + IRQ.
- AMD piix4 driver as best-effort behind feature flag.

### T3. /dev/i2c-N userspace ABI — `kernel/src/i2c/dev.rs`
- ioctls compatible with Linux i2c-dev (`I2C_RDWR`, `I2C_SLAVE`).

### T4. GPIO chardev — `kernel/src/gpio/`
- `/dev/gpiochip*`; `GpioChip`, `GpioLine`.
- Chardev v2 ABI: `GPIO_V2_GET_LINE_IOCTL`, `GPIO_V2_LINE_GET_VALUES_IOCTL`, `GPIO_V2_LINE_SET_VALUES_IOCTL`.
- Line-event fd for edge detection.

### T5. hwmon class — `kernel/src/hwmon/`
- `HwmonDevice` registration; sysfs attribute groups: `tempN_*`, `fanN_*`, `inN_*`, `powerN_*`, `currN_*`, `humN_*`.
- `hwmon_device_register_with_groups`-class API for drivers.

### T6. Common sensor drivers — `kernel/src/hwmon/drivers/`
- `coretemp` (Intel package temp via MSR, no I²C).
- `nct6775` (super-IO chip on many boards).
- `acpi_battery` and `acpi_thermal` already in §23; bridge into hwmon.

### T7. willosensors — `userspace/willosensors/`
- Walk `/sys/class/hwmon/`; print named groups.
- Config in `/etc/sensors3.conf`-class (compat).

### T8. willofancontrol — `userspace/willofancontrol/`
- Loop: read temp, compute target PWM via curve, write PWM sysfs.
- Hysteresis + min/max PWM + emergency full speed.

### T9. Settings UI — `userspace/willoc-settings/plugins/sensors.rs`
- Live sensor view; fan curve editor.

## New / modified files

| Path | Change |
| --- | --- |
| `kernel/src/i2c/*` | **new** module |
| `kernel/src/gpio/*` | **new** module |
| `kernel/src/hwmon/*` | **new** module |
| `userspace/willosensors/` | **new** |
| `userspace/willofancontrol/` | **new** |
| `userspace/willoc-settings/plugins/sensors.rs` | **new** |

## Tests to add

- `kernel/tests/i2c_smbus_basic.rs` — read/write byte against fake sensor.
- `kernel/tests/gpio_chardev.rs` — request line; toggle; verify event fd.
- `kernel/tests/hwmon_register.rs` — register chip; sysfs attributes appear.
- `userspace/willofancontrol/tests/curve_apply.rs` — temp 50→70 raises PWM expectedly.

## Risks & open questions

- **Sensor chip zoo** — there are hundreds; ship the top ten chips that cover ~90% of consumer laptops.
- **Fan stall** — too-low PWM can stall; document min PWM per chip.
- **GPIO security** — exposing chardev to userspace is a foot-gun; default profile in §39 only grants to `gpio` group.
- **I²C bus contention** — SMBus is shared; lm-sensors loops can stall a busy bus; rate-limit.
- **ACPI vs hwmon** — some laptop sensors are only readable via ACPI methods, not I²C; bridge.
- **Real-time constraints** — fancontrol loops must not starve under §44 RT load; run in a low-priority thread.
