use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

pub const SOCKET_PATH: &str = "/run/t2newfanrd.sock";

#[derive(Debug, Default)]
pub struct ManualState {
    /// Per-fan targets in RPM; `None` means the automatic temperature curve.
    pub targets: Option<Vec<u32>>,
    /// Bumped on every change so the daemon loop cannot miss a wakeup.
    pub version: u64,
}

pub type Control = Arc<(Mutex<ManualState>, Condvar)>;

pub fn new_control() -> Control {
    Arc::new((Mutex::new(ManualState::default()), Condvar::new()))
}

pub fn set_manual_targets(control: &Control, targets: Vec<u32>) {
    let mut state = control.0.lock().unwrap();
    state.targets = Some(targets);
    state.version = state.version.wrapping_add(1);
    control.1.notify_all();
}

pub fn set_auto(control: &Control) {
    let mut state = control.0.lock().unwrap();
    state.targets = None;
    state.version = state.version.wrapping_add(1);
    control.1.notify_all();
}

pub fn status(control: &Control) -> String {
    let state = control.0.lock().unwrap();
    match &state.targets {
        Some(targets) => {
            let values = targets
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("MANUAL {values}")
        }
        None => "AUTO".to_owned(),
    }
}

pub fn spawn_server(control: Control, fan_count: usize) -> std::io::Result<thread::JoinHandle<()>> {
    let _ = std::fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH)?;
    std::fs::set_permissions(SOCKET_PATH, std::fs::Permissions::from_mode(0o666))?;

    Ok(thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let control = control.clone();
                    thread::spawn(move || {
                        if let Err(err) = handle_client(stream, control, fan_count) {
                            eprintln!("control client error: {err}");
                        }
                    });
                }
                Err(err) => eprintln!("control socket error: {err}"),
            }
        }
    }))
}

fn handle_client(
    mut stream: UnixStream,
    control: Control,
    fan_count: usize,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let line = line.trim();

    let reply = match line {
        "STATUS" => status(&control),
        "AUTO" => {
            set_auto(&control);
            "OK".to_owned()
        }
        _ if line.starts_with("SET ") => {
            let values = line[4..]
                .split_whitespace()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>();
            match values {
                Ok(values) if values.len() == fan_count => {
                    set_manual_targets(&control, values);
                    "OK".to_owned()
                }
                Ok(_) => format!("ERR expected {fan_count} fan targets"),
                Err(_) => "ERR invalid target".to_owned(),
            }
        }
        _ => "ERR unknown command".to_owned(),
    };

    stream.write_all(reply.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn send_command(command: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    Ok(reply.trim().to_owned())
}
