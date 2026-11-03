# t2newfanrd

A patched fork of [T2FanRD](https://github.com/GnomedDev/T2FanRD) for newer
Linux kernels on T2 Macs, with an optional system tray frontend for setting
fan speeds manually.

The original daemon looks for the legacy `apple-ib-tb` files (`fanX_manual` and
`fanX_output`). Current kernels expose Apple SMC fans through hwmon instead
(`fanX_input`, `fanX_min`, `fanX_max`, `fanX_target`), and writing a target is
disabled unless the `macsmc-hwmon` module is loaded with `fan_control=1`.
Without these changes the daemon exits at startup and systemd eventually reports
`start-limit-hit`.

## Features

- Automatic fan curve driven by the higher of the CPU and GPU temperatures.
- Per-fan system tray sliders for setting any speed between `fanX_min` and
  `fanX_max`, plus an **Automatic fan curve** switch.
- Both fans may live on one hwmon node or be split across several nodes.
- The original fan target is saved at startup and restored on shutdown.
- Installable as `t2newfanrd` with its own binary and systemd unit, so a distro
  `t2fanrd` package cannot overwrite it.
- `/etc/t2fand.conf` is compatible with upstream T2FanRD.

## Requirements

- A T2 Mac running Linux.
- A kernel with the `macsmc-hwmon` driver (`modinfo macsmc-hwmon`).
- `macsmc-hwmon` loaded with `fan_control=1`.
- For the tray frontend: GTK4, and on GNOME the AppIndicator extension
  (`gnome-shell-extension-appindicator`) enabled.

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

The package installs the daemon, the tray binary, icons, and an XDG autostart
entry (`/etc/xdg/autostart/t2newfanrd-tray.desktop`). The tray starts at the next
login.

### Manual build

```bash
git clone https://github.com/Anthony2cc/t2newfanrd.git
cd t2newfanrd
cargo build --release --locked

sudo install -Dm755 target/release/t2newfanrd /usr/bin/t2newfanrd
sudo install -Dm755 target/release/t2newfanrd-tray /usr/bin/t2newfanrd-tray
sudo install -Dm644 packaging/t2newfanrd.service /usr/lib/systemd/system/t2newfanrd.service
sudo install -Dm644 packaging/t2fand.conf /etc/t2fand.conf
sudo systemctl daemon-reload
sudo systemctl enable --now t2newfanrd
```

For desktop integration, also install the icons and autostart entry:

```bash
sudo install -Dm644 assets/t2newfanrd.svg \
  /usr/share/icons/hicolor/scalable/apps/t2newfanrd.svg
sudo install -Dm644 assets/t2newfanrd-symbolic.svg \
  /usr/share/icons/hicolor/scalable/apps/t2newfanrd-symbolic.svg
sudo install -Dm644 packaging/t2newfanrd-tray.desktop \
  /usr/share/applications/t2newfanrd-tray.desktop
sudo install -Dm644 packaging/t2newfanrd-tray-autostart.desktop \
  /etc/xdg/autostart/t2newfanrd-tray.desktop
sudo gtk-update-icon-cache /usr/share/icons/hicolor || true
```

### Check status

```bash
systemctl status t2newfanrd
cat /sys/class/hwmon/hwmon*/fan*_target
```

## Usage

Start the tray in the current session (or let it autostart at login):

```bash
t2newfanrd-tray            # open the control window
t2newfanrd-tray --hidden   # tray icon only
```

Clicking the tray icon or choosing **Open fan controls** opens a window with:

- **Automatic fan curve** — the daemon follows `/etc/t2fand.conf`.
- **Fan sliders** — one per fan, in RPM, clamped to each fan's `min`/`max`
  values. Moving a slider switches the daemon to manual mode immediately.
- **Actual speed** — the current `fanX_input` reading next to each slider.
- **Quit** — closes the tray application; the daemon keeps running.

Manual speeds stay in effect until **Automatic fan curve** is re-enabled or the
daemon is restarted.

The tray and the daemon communicate over a local Unix socket,
`/run/t2newfanrd.sock`. The socket is world-writable so the desktop user can use
it without extra group setup; on a multi-user machine, any local user can change
fan speeds. Tighten the socket permissions or run the tray only for trusted
users if that matters to you.

## Configuration

`/etc/t2fand.conf` is read when the daemon starts and applies to automatic mode:

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
