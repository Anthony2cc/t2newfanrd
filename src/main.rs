#![warn(rust_2018_idioms)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::similar_names,
    clippy::module_name_repetitions
)]

use std::{
    io::{ErrorKind, Read, Seek},
    process::ExitCode,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use arraydeque::ArrayDeque;
use fan_controller::FanController;
use nonempty::NonEmpty as NonEmptyVec;
use signal_hook::consts::{SIGINT, SIGTERM};
use t2newfanrd::{hwmon, ipc};

use config::load_fan_configs;
use error::{Error, Result};

mod config;
mod error;
mod fan_controller;

#[cfg(not(target_os = "linux"))]
compile_error!("This tool is only developed for Linux systems.");

#[cfg(debug_assertions)]
const PID_FILE: &str = "t2newfanrd.pid";
#[cfg(not(debug_assertions))]
const PID_FILE: &str = "/run/t2newfanrd.pid";

fn get_current_euid() -> libc::uid_t {
    // SAFETY: FFI call with no preconditions
    unsafe { libc::geteuid() }
}

fn check_pid_file() -> Result<()> {
    match std::fs::read_to_string(PID_FILE) {
        Ok(pid) => {
            let mut proc_path = std::path::PathBuf::new();
            proc_path.push("/proc");
            proc_path.push(pid);

            if proc_path.exists() {
                return Err(Error::AlreadyRunning);
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(Error::PidRead(err)),
    }

    let current_pid = std::process::id().to_string();
    std::fs::write(PID_FILE, current_pid).map_err(Error::PidWrite)
}

fn read_temp_file(temp_file: &mut std::fs::File, temp_buf: &mut String) -> Result<u8> {
    temp_file
        .read_to_string(temp_buf)
        .map_err(Error::TempRead)?;

    temp_file.rewind().map_err(Error::TempSeek)?;

    let temp = temp_buf.trim_end().parse::<u32>().map_err(Error::TempParse);
    temp_buf.clear();
    temp.map(|t| (t / 1000) as u8)
}

fn find_temp_file(temps: glob::Paths, temp_buf: &mut String) -> Option<std::fs::File> {
    for temp_path_res in temps {
        let Ok(temp_path) = temp_path_res else {
            eprintln!("Unable to read glob path");
            continue;
        };

        let Ok(mut temp_file) = std::fs::File::open(temp_path) else {
            eprintln!("Unable to open temperature sensor");
            continue;
        };

        if read_temp_file(&mut temp_file, temp_buf).is_ok() {
            return Some(temp_file);
        }
    }

    None
}

fn find_cpu_temp_file(temp_buf: &mut String) -> Result<std::fs::File> {
    let temps = glob::glob("/sys/devices/platform/coretemp.0/hwmon/hwmon*/temp1_input")?;
    find_temp_file(temps, temp_buf).ok_or(Error::NoCpu)
}

fn find_gpu_temp_file(temp_buf: &mut String) -> Result<Option<std::fs::File>> {
    // The amdgpu node is not necessarily card0; scan every DRM card.
    let temps = glob::glob("/sys/class/drm/card*/device/hwmon/hwmon*/temp1_input")?;
    Ok(find_temp_file(temps, temp_buf))
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn start_temp_loop(
    mut temp_buffer: String,
    mut cpu_temp_file: std::fs::File,
    mut gpu_temp_file: Option<std::fs::File>,
    fans: &NonEmptyVec<FanController>,
    control: ipc::Control,
) -> Result<()> {
    let cancellation_token = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, cancellation_token.clone()).map_err(Error::Signal)?;
    signal_hook::flag::register(SIGTERM, cancellation_token.clone()).map_err(Error::Signal)?;

    let mut temps = ArrayDeque::<u8, 50, arraydeque::Wrapping>::new();
    let mut applied: Option<Vec<u32>> = None;

    while !cancellation_token.load(std::sync::atomic::Ordering::Relaxed) {
        let cpu_temp = read_temp_file(&mut cpu_temp_file, &mut temp_buffer)?;
        let temp = if let Some(gpu_temp_file) = &mut gpu_temp_file {
            let gpu_temp = read_temp_file(gpu_temp_file, &mut temp_buffer)?;
            if gpu_temp > cpu_temp {
                gpu_temp
            } else {
                cpu_temp
            }
        } else {
            cpu_temp
        };

        temps.push_back(temp);
        let sum_temp: u16 = temps.iter().map(|t| *t as u16).sum();
        let mean_temp = sum_temp / (temps.len() as u16);

        let (manual, version) = {
            let state = control.0.lock().unwrap();
            (state.targets.clone(), state.version)
        };

        let desired: Vec<u32> = match manual {
            Some(targets) if targets.len() == fans.len() => targets,
            _ => fans
                .iter()
                .map(|fan| fan.calc_speed(mean_temp as u8))
                .collect(),
        };

        if applied.as_ref() != Some(&desired) {
            for (fan, speed) in fans.iter().zip(desired.iter().copied()) {
                fan.set_speed(speed)?;
            }
            applied = Some(desired);
        }

        let guard = control.0.lock().unwrap();
        if guard.version == version {
            let _ = control
                .1
                .wait_timeout(guard, Duration::from_secs(1))
                .unwrap();
        }
    }

    Ok(())
}

fn real_main() -> Result<()> {
    if get_current_euid() != 0 {
        return Err(Error::NotRoot);
    }

    check_pid_file()?;

    let mut temp_buffer = String::new();

    let fan_paths = NonEmptyVec::from_vec(hwmon::find_fan_paths()).ok_or(Error::NoFan)?;
    let fans = load_fan_configs(fan_paths)?;
    let cpu_temp_file = find_cpu_temp_file(&mut temp_buffer)?;
    let gpu_temp_file = find_gpu_temp_file(&mut temp_buffer)?;

    let control = ipc::new_control();
    ipc::spawn_server(control.clone(), fans.len()).map_err(Error::ControlSocket)?;

    println!();
    let res = start_temp_loop(temp_buffer, cpu_temp_file, gpu_temp_file, &fans, control);
    println!("T2 Fan Daemon is shutting down...");
    let _ = std::fs::remove_file(ipc::SOCKET_PATH);
    for fan in &fans {
        fan.restore()?;
    }

    let pid_res = std::fs::remove_file(PID_FILE).map_err(Error::PidDelete);
    match (res, pid_res) {
        (Err(err), _) | (_, Err(err)) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
    }
}
