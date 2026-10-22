use hidapi::{HidApi, HidDevice};
use std::{
    env,
    process::ExitCode,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

const VENDOR_ID: u16 = 0x340d;
const PRODUCT_ID: u16 = 0x1710;
const REFRESH: Duration = Duration::from_secs(3);
const HELP: &str = "GIANTS Software USB beacon

Usage: beacon <on|round|blink|off> [--duration SECONDS | --hold]
       beacon list
       beacon --help
       beacon --version

  on, round    Rotate the light
  blink        Flash the light
  off          Turn the light off
  list         Show connected GIANTS beacons (does not change the light)
  --duration   Stay on for a positive number of seconds, then turn off
  --hold       Stay on until Ctrl+C (or SIGTERM on Unix), then turn off

Without --duration or --hold, send once and exit immediately.
The beacon then turns itself off after about 10 seconds.
Only one timed/held command should run at a time; stop it before using off.";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Round,
    Blink,
    Off,
}

impl Mode {
    fn report(self) -> [u8; 10] {
        // Exact output reports from reference-project/giants_beacon.py.
        // The leading zero is the HID report ID, not part of the payload.
        match self {
            Self::Round => [0x00, 0xFF, 0x01, 0x66, 0xC8, 0xFF, 0xAD, 0x52, 0x81, 0xD6],
            Self::Blink => [0x00, 0xFF, 0x07, 0xFF, 0x64, 0xFF, 0xEB, 0x7D, 0x9A, 0x03],
            Self::Off => [0x00, 0xFF, 0x00, 0x00, 0x64, 0x00, 0x32, 0x9E, 0xD7, 0x0D],
        }
    }
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    List,
    Once(Mode),
    Timed(Mode, Option<Duration>),
}

fn parse(args: &[&str]) -> Result<Command, String> {
    let (name, options) = match args {
        [] | ["--help" | "-h"] => return Ok(Command::Help),
        ["--version" | "-V"] => return Ok(Command::Version),
        ["list"] => return Ok(Command::List),
        [name, options @ ..] => (*name, options),
    };
    let mode = match name {
        "on" | "round" => Mode::Round,
        "blink" => Mode::Blink,
        "off" => Mode::Off,
        _ => return Err(format!("unknown command {name:?}; see beacon --help")),
    };
    match options {
        [] => Ok(Command::Once(mode)),
        _ if mode == Mode::Off => Err("off does not accept options".into()),
        ["--hold"] => Ok(Command::Timed(mode, None)),
        ["--duration", seconds] => {
            let duration = seconds
                .parse::<f64>()
                .ok()
                .and_then(|n| Duration::try_from_secs_f64(n).ok())
                .filter(|d| !d.is_zero() && Instant::now().checked_add(*d).is_some())
                .ok_or(
                    "duration must be a finite, positive number of seconds within timer range",
                )?;
            Ok(Command::Timed(mode, Some(duration)))
        }
        _ => Err("expected --duration SECONDS or --hold; see beacon --help".into()),
    }
}

fn write(device: &HidDevice, mode: Mode) -> Result<(), String> {
    let report = mode.report();
    let written = device
        .write(&report)
        .map_err(|e| format!("USB write failed: {e}"))?;
    // The Windows backend pads reports and may return zero on synchronous success.
    if !cfg!(target_os = "windows") && written != report.len() {
        return Err(format!(
            "incomplete USB write: {written}/{} bytes",
            report.len()
        ));
    }
    Ok(())
}

fn keep_lit(
    mode: Mode,
    duration: Option<Duration>,
    stop: Receiver<()>,
    mut send: impl FnMut(Mode) -> Result<(), String>,
) -> Result<(), String> {
    let start = Instant::now();
    let result = (|| {
        loop {
            send(mode)?;
            let wait = duration.map_or(REFRESH, |d| d.saturating_sub(start.elapsed()).min(REFRESH));
            match stop.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if duration.is_some_and(|d| start.elapsed() >= d) {
                break;
            }
        }
        Ok(())
    })();
    // Always try to switch off, including after a failed refresh.
    let off = send(Mode::Off);
    result.and(off)
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Help => {
            println!("{HELP}");
            return Ok(());
        }
        Command::Version => {
            println!("beacon {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    let api = HidApi::new().map_err(|e| format!("cannot initialize HID: {e}"))?;
    if command == Command::List {
        let mut found = false;
        for device in api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        {
            println!(
                "340d:1710  {}  {}",
                device.product_string().unwrap_or("GIANTS beacon"),
                device.path().to_string_lossy()
            );
            found = true;
        }
        if !found {
            println!("No GIANTS beacon (340d:1710) connected.");
        }
        return Ok(());
    }

    let device = api.open(VENDOR_ID, PRODUCT_ID).map_err(|e| {
        let mut message = format!("cannot open GIANTS beacon (340d:1710): {e}\nCheck its USB connection and close other apps using it.");
        if cfg!(target_os = "linux") {
            message.push_str("\nOn Linux, install 70-giants-beacon.rules as described in README.md, then reconnect the beacon.");
        }
        message
    })?;
    match command {
        Command::Once(mode) => write(&device, mode),
        Command::Timed(mode, duration) => {
            let (tx, rx) = mpsc::channel();
            ctrlc::set_handler(move || {
                let _ = tx.send(());
            })
            .map_err(|e| format!("cannot install shutdown handler: {e}"))?;
            keep_lit(mode, duration, rx, |mode| write(&device, mode))
        }
        _ => unreachable!(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let command = match parse(&args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("beacon: {error}");
            return ExitCode::from(2);
        }
    };
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("beacon: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        for (args, expected) in [
            (vec![], Command::Help),
            (vec!["--help"], Command::Help),
            (vec!["--version"], Command::Version),
            (vec!["list"], Command::List),
            (vec!["on"], Command::Once(Mode::Round)),
            (vec!["round"], Command::Once(Mode::Round)),
            (vec!["blink"], Command::Once(Mode::Blink)),
            (vec!["off"], Command::Once(Mode::Off)),
            (vec!["blink", "--hold"], Command::Timed(Mode::Blink, None)),
            (
                vec!["on", "--duration", "0.25"],
                Command::Timed(Mode::Round, Some(Duration::from_millis(250))),
            ),
        ] {
            assert_eq!(parse(&args).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_invalid_input_before_opening_usb() {
        for args in [
            vec!["invalid"],
            vec!["off", "--hold"],
            vec!["on", "--duration"],
            vec!["on", "--hold", "--duration", "1"],
            vec!["list", "extra"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        for seconds in ["0", "-1", "NaN", "inf", "1e100", "1e-100", "abc"] {
            assert!(
                parse(&["blink", "--duration", seconds]).is_err(),
                "{seconds}"
            );
        }
    }

    #[test]
    fn turns_off_when_interrupted_or_duration_expires() {
        for interrupted in [true, false] {
            let (tx, rx) = mpsc::channel();
            if interrupted {
                tx.send(()).unwrap();
            }
            let mut sent = Vec::new();
            keep_lit(Mode::Blink, Some(Duration::from_millis(1)), rx, |mode| {
                sent.push(mode);
                Ok(())
            })
            .unwrap();
            assert_eq!(sent, [Mode::Blink, Mode::Off]);
        }
    }

    #[test]
    fn refreshes_until_stopped() {
        let (tx, rx) = mpsc::channel();
        let mut sent = Vec::new();
        keep_lit(Mode::Round, None, rx, |mode| {
            sent.push(mode);
            if sent.len() == 2 {
                tx.send(()).unwrap();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(sent, [Mode::Round, Mode::Round, Mode::Off]);
    }

    #[test]
    fn attempts_off_after_write_failure_and_reports_errors() {
        let (_tx, rx) = mpsc::channel();
        let mut sent = Vec::new();
        let error = keep_lit(Mode::Round, None, rx, |mode| {
            sent.push(mode);
            Err("USB disconnected".into())
        })
        .unwrap_err();
        assert_eq!(sent, [Mode::Round, Mode::Off]);
        assert_eq!(error, "USB disconnected");

        let (tx, rx) = mpsc::channel();
        tx.send(()).unwrap();
        let error = keep_lit(Mode::Round, None, rx, |mode| {
            if mode == Mode::Off {
                Err("off failed".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error, "off failed");
    }
}
