# GIANTS USB beacon

A small Rust command-line tool for the Farming Simulator GIANTS Software USB beacon
(`340d:1710`). Build once and run a single executable.

## Build and run

With Rust and a platform linker installed:

```sh
cargo build --release --locked
./target/release/beacon on
```

The executable is `target/release/beacon` on Linux/macOS and
`target\release\beacon.exe` on Windows. Build on the platform where you will use it.
Linux uses a native Rust HID backend without a libusb/libudev development package;
Windows uses the native HID API. macOS builds also need the Xcode command-line tools.

To install into Cargo's executable directory (usually `~/.cargo/bin`):

```sh
cargo install --path . --locked
beacon blink
```

Alternatively, on Linux, link this project's compiled executable into `/usr/bin`:

```sh
./install.sh
beacon blink
```

The script requests sudo access if needed and creates `/usr/bin/beacon` pointing
to this project's `target/release/beacon`. It can be invoked from any directory,
is safe to rerun, and refuses to replace an existing file or a different link.
Keep the project in place; rebuilding updates the linked executable, while moving
the project or running `cargo clean` breaks the link until the executable is restored.
Use `command -v beacon` to check which installation your PATH selects; an earlier
Cargo installation may take precedence over `/usr/bin/beacon`.

## Linux: allow access once

The beacon may be visible in `beacon list` but still need permission to control it.
From this project directory, install the supplied device-specific udev rule:

```sh
sudo install -m 644 70-giants-beacon.rules /etc/udev/rules.d/70-giants-beacon.rules
sudo udevadm control --reload-rules
```

Unplug and reconnect the beacon. Your active local desktop user can then run
`beacon on` without sudo. This rule grants access only to this USB vendor/product;
it does not grant access to background system services or remote-only sessions.

## Commands

```sh
beacon on                      # Rotating light; returns immediately
beacon round                   # Same as on
beacon blink                   # Blinking light; returns immediately
beacon off                     # Switch off immediately
beacon blink --duration 5       # Blink for 5 seconds, switch off, exit
beacon on --duration 0.5        # Fractional seconds are supported
beacon on --hold                # Keep rotating until Ctrl+C
beacon list                    # List connected beacons without changing them
beacon --help
beacon --version
```

The hardware turns itself off about 10 seconds after the last on/blink command.
Without options, the program sends one command and exits, making it suitable for
notification hooks. Repeated notifications restart that hardware timeout.

`--duration` and `--hold` keep a process running and resend the command every
3 seconds. They send `off` when the duration ends, on Ctrl+C, or on SIGTERM/SIGHUP
on Unix. A forced kill or USB disconnection cannot send `off`; the hardware timeout
is the fallback. Only run one timed/held command at a time: another `beacon off`
does not stop its refresh loop. Stop that process to acknowledge the notification.

Commands control the first matching beacon. Success is quiet, errors go to stderr.
Exit codes: `0` success, `1` USB/runtime error, `2` invalid arguments.
`list` exits successfully even if no beacon is connected; listing does not prove
write access. This tool does not query the current light state.

## Notification hooks

Configure an application's command/script hook to invoke the installed executable
by its absolute path. For example, a Codex workflow or an email notification hook
can invoke:

```sh
/home/your-user/.cargo/bin/beacon blink
```

For a shorter alert, use `beacon blink --duration 2` (this waits for 2 seconds).
The executable controls the light; the application or your script decides when
to call it. No email account or application settings are changed by this project.

## Development

```sh
cargo test --locked
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

The implementation is in `src/main.rs`. Its only direct dependencies are
[`hidapi`](https://docs.rs/hidapi/2.6.7/hidapi/) for USB HID access and
[`ctrlc`](https://docs.rs/ctrlc/latest/ctrlc/) for clean shutdown of timed/held alerts.
