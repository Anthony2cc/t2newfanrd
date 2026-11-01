# t2newfanrd

A patched fork of [T2FanRD](https://github.com/GnomedDev/T2FanRD) that works with
the `macsmc-hwmon` fan interface used by newer Linux kernels on T2 Macs.

The original daemon looks for the legacy `apple-ib-tb` files (`fanX_manual` and
`fanX_output`). Current kernels expose Apple SMC fans through hwmon instead
(`fanX_input`, `fanX_min`, `fanX_max`, `fanX_target`), and writing a target is
disabled unless the `macsmc-hwmon` module is loaded with `fan_control=1`.
Without these changes the daemon exits at startup and systemd eventually reports
`start-limit-hit`.

## What changed

- Fan discovery through `/sys/class/hwmon/hwmon*/name`, limited to Apple SMC
  hwmon nodes. Both fans may live on one node or be split across several nodes.
- Speed control by writing `fanX_target` (RPM) instead of `fanX_manual` /
  `fanX_output`.
- The original fan target is saved at startup and restored on SIGTERM/SIGINT.
- GPU temperature lookup scans every DRM card instead of hardcoding `card0`.
- Packaged as `t2newfanrd` with its own binary and systemd unit, so a distro
  `t2fanrd` package cannot overwrite it.

## Requirements

- A T2 Mac running Linux.
- A kernel with the `macsmc-hwmon` driver (`modinfo macsmc-hwmon`).
- `macsmc-hwmon` loaded with `fan_control=1`.

Enable fan control:

```bash
echo "options macsmc-hwmon fan_control=1" | sudo tee /etc/modprobe.d/macsmc-hwmon.conf
sudo modprobe -r macsmc-hwmon
sudo modprobe macsmc-hwmon
```

Verify that each fan has a writable target:

```bash
grep -H . /sys/class/hwmon/hwmon*/name
ls -l /sys/class/hwmon/hwmon*/fan*
```

Writing `fanX_target` while fan control is disabled returns `EOPNOTSUPP`.

> `fan_control=1` taints the kernel ("Setting dangerous option"). This is
> expected: the driver is taking over fan management from the SMC.

## Installation

### Arch Linux / CachyOS (package)

```bash
git clone https://github.com/Anthony2cc/t2newfanrd.git
cd t2newfanrd/packaging
makepkg -si
sudo systemctl enable --now t2newfanrd
```

If the distribution `t2fanrd` package is installed, remove or disable it first:

```bash
sudo systemctl disable --now t2fanrd
sudo pacman -Rns t2fanrd
```

### Manual build

```bash
git clone https://github.com/Anthony2cc/t2newfanrd.git
cd t2newfanrd
cargo build --release --locked

sudo install -Dm755 target/release/t2fanrd /usr/bin/t2newfanrd
sudo install -Dm644 packaging/t2newfanrd.service /usr/lib/systemd/system/t2newfanrd.service
sudo systemctl daemon-reload
sudo systemctl enable --now t2newfanrd
```

### Check status

```bash
systemctl status t2newfanrd
cat /sys/class/hwmon/hwmon*/fan*_target
```

## Configuration

`/etc/t2fand.conf` is compatible with upstream T2FanRD:

```ini
[Fan1]
low_temp = 45
high_temp = 75
speed_curve = linear
always_full_speed = false

[Fan2]
low_temp = 45
high_temp = 75
speed_curve = linear
always_full_speed = false
```

| Key | Description |
| --- | --- |
| `low_temp` | At or below this temperature the fan runs at `fanX_min`. |
| `high_temp` | At or above this temperature the fan runs at `fanX_max`. |
| `speed_curve` | `linear`, `exponential` or `logarithmic`. |
| `always_full_speed` | Ignore temperature and always use `fanX_max`. |

The daemon uses the higher of the CPU (`coretemp`) and GPU (`amdgpu`)
temperatures and averages the last 50 samples. If the configuration file is
missing, a default is generated on first start.

## Uninstall

```bash
sudo systemctl disable --now t2newfanrd
sudo pacman -Rns t2newfanrd   # package installation
```

## Credits

- Original project: [T2FanRD](https://github.com/GnomedDev/T2FanRD) by
  [GnomedDev](https://github.com/GnomedDev). This repository is a fork; the fan
  curve logic, configuration format and overall design come from the original
  project.
- `macsmc-hwmon` is part of the Linux kernel, written by James Calligeros and
  other contributors.

## License

GPL-3.0-or-later, same as the original T2FanRD. See [LICENCE](LICENCE).

Fan control touches hardware; use at your own risk.
