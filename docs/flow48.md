# flow48 — M48: architecture & runtime flows (Sensors / I²C / GPIO / hwmon)

> Derived from `docs/idea.md` §7 and the work48 plan.

## Component map

```
        userspace
        +-------------+   +-----------------+
        | willosensors|   | willofancontrol |
        +------+------+   +--------+--------+
               | sysfs reads      | PWM writes + temp reads
               v                  v
        +----------------------------------+
        |     kernel hwmon class           |
        +-----+----------+-----------------+
              |          |
              v          v
        +---------+   +--------+
        | i2c bus |   | coretemp|
        +----+----+   +--------+
             |
             v
        +---------+
        | i801    | (SMBus host)
        +----+----+
             |
             v
        I²C devices: nct6775, sht35, ina219, ...

        +-----------+
        | gpiochip0 | -> /dev/gpiochip0 chardev
        +-----------+
```

## I²C transfer (read SMBus byte data)

```
i2c_smbus_read_byte_data(client=0x76, reg=0x00)
  -> i2c_transfer:
       msg0: addr=0x76, len=1, buf=[0x00], flags=write
       msg1: addr=0x76, len=1, buf=*out, flags=read
  -> host i801:
       acquire bus
       program HSTCMD, HSTDA0
       start
       wait HSTSTS done
       read HSTDA0
       release bus
  -> return byte
```

## GPIO chardev request + read

```
fd = open("/dev/gpiochip0", O_RDWR)
ioctl(fd, GPIO_V2_GET_LINE_IOCTL, {
   offsets=[3], num_lines=1,
   config={ flags=GPIO_V2_LINE_FLAG_INPUT | EDGE_RISING },
   consumer="lid-sensor"
}) -> line_fd
ioctl(line_fd, GPIO_V2_LINE_GET_VALUES_IOCTL, &vals) -> bit
poll(line_fd, POLLIN); read(line_fd, &event)
  event = { offset, timestamp_ns, id=RISING|FALLING, line_seqno }
```

## hwmon registration (driver side)

```
let dev = hwmon_device_register_with_groups("nct6775", &[
    HwmonGroup { name: "fan1",  attrs: &[FAN_INPUT, FAN_MIN, FAN_TARGET] },
    HwmonGroup { name: "temp1", attrs: &[TEMP_INPUT, TEMP_LABEL, TEMP_MAX] },
    HwmonGroup { name: "pwm1",  attrs: &[PWM_INPUT, PWM_ENABLE, PWM_AUTO_POINT_TEMP] },
]);
sysfs nodes appear:
  /sys/class/hwmon/hwmon0/
    name -> "nct6775"
    fan1_input  -> 1234 (rpm)
    temp1_input -> 56000 (millidegC)
    pwm1        -> 128 (0..255)
```

## willosensors print loop

```
willosensors:
  for d in glob("/sys/class/hwmon/hwmon*/"):
     name = read d/name
     for f in d/temp*_input:
        labelf = f.replace(_input, _label)
        unit = "°C"
        v = read f / 1000
        print f"{name}.{label}: {v}{unit}"
     similar for fan, in, power
```

## willofancontrol loop

```
config:
  fan1 -> sensor temp1; min_pwm 60; max_pwm 200; t_min 40; t_max 75
loop 1Hz (low priority):
  t = read temp1_input / 1000
  pwm = lerp(t_min..t_max, min_pwm..max_pwm)
  apply hysteresis (don't change unless |Δ| > 3%)
  write pwm1 = pwm
  emergency: if t > 90: write pwm1 = 255; persist for 60s
```

## §36 monitor integration

```
willomonitor 1Hz:
  read all hwmon devices into ProcSample.temp/fan map
  graph component renders rolling 5-min view
fan curve editor (§36 sensors plugin):
  visualises curve; on save -> /etc/willo/fancontrol.conf reload
```

## Failure paths

- **I²C transaction timeout** → driver returns `-EREMOTEIO`; userland reads error; logged.
- **Sensor reads garbage** → driver clamps to "n/a" if value out of plausible range.
- **PWM underruns to 0** → fan stalls; set `PWM_MIN` per chip; never write below.
- **GPIO line already requested** → ioctl returns `-EBUSY`; show consumer in `/sys/kernel/debug/gpio`.
- **Bus arbitration** → SMBus host driver retries; after 3 fails surfaces error.

## Data structures

```rust
pub struct I2cMsg {
    pub addr: u16,
    pub flags: I2cFlags,                     // Read | Write | TenBit | NoStart
    pub len: u16,
    pub buf: BufRef,
}

pub trait I2cAdapter {
    fn xfer(&self, msgs: &mut [I2cMsg]) -> Result<usize, Errno>;
    fn smbus(&self, addr: u16, op: SmbusOp) -> Result<SmbusResult, Errno>;
}

pub struct GpioLine {
    pub chip: GpioChipId,
    pub offset: u32,
    pub flags: GpioLineFlags,                // Input/Output, Active{High,Low}, Edge{Rising,Falling}
    pub consumer: SmolStr,
}

pub struct HwmonDevice {
    pub name: SmolStr,
    pub groups: Vec<HwmonGroup>,
    pub parent: Device,
}

pub enum HwmonAttr {
    TempInput, TempLabel, TempMax, TempMin, TempAlarm,
    FanInput, FanMin, FanTarget, FanLabel,
    PwmInput, PwmEnable, PwmAutoPointTemp,
    InInput, InLabel,
    PowerInput, CurrInput, HumidityInput,
}
```
