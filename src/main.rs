#![cfg_attr(all(target_os = "windows"), windows_subsystem = "windows")]
#![cfg_attr(
    all(target_os = "windows"),
    feature(panic_update_hook, internal_output_capture)
)]
#![feature(panic_backtrace_config)]

#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        cfg_if::cfg_if! {
            if #[cfg(target_family = "windows")] {
                crate::logging_windows::logging($prefix, std::format_args!($($arg)*));
            } else {
                std::println!("[{}]: {}", $prefix, std::format_args!($($arg)*));
            }
        };

    };
}

#[macro_use]
extern crate rocket;
extern crate core;

use lazy_static::lazy_static;

use std::{
    env, fs,
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};
use chrono::{FixedOffset, TimeZone, Utc};

pub mod controller;
pub mod easytier;
pub mod server;
pub mod scaffolding;

#[cfg(target_family = "windows")]
pub mod logging_windows;

#[cfg(target_os = "macos")]
pub mod ui_macos;

pub const MOTD: &'static str = "§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）";

#[cfg(target_family = "windows")]
pub mod lock_windows;
#[cfg(target_family = "windows")]
use lock_windows::State as Lock;
#[cfg(target_family = "unix")]
pub mod lock_unix;
mod mc;
mod ports;

#[cfg(target_family = "unix")]
use lock_unix::State as Lock;

lazy_static::lazy_static! {
    static ref ADDRESSES: Vec<IpAddr> = {
        let mut addresses: Vec<IpAddr> = vec![];

        if let Ok(networks) = local_ip_address::list_afinet_netifas() {
            logging!("UI", "Local IP Addresses: {:?}", networks);

            for (_, address) in networks.into_iter() {
                match address {
                    IpAddr::V4(ip) => {
                        let parts = ip.octets();
                        if !(parts[0] == 10 && parts[1] == 144 && parts[2] == 144) && ip != Ipv4Addr::LOCALHOST && ip != Ipv4Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    },
                    IpAddr::V6(ip) => {
                        if ip != Ipv6Addr::LOCALHOST && ip != Ipv6Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    }
                };
            }
        }

        addresses.push(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        addresses.push(IpAddr::V6(Ipv6Addr::UNSPECIFIED));

        addresses.sort_by(|ip1, ip2| ip2.cmp(ip1));
        addresses
    };
}

lazy_static! {
    pub static ref FILE_ROOT: std::path::PathBuf = {
        let path = if cfg!(target_os = "macos")
            && let Ok(home) = env::var("HOME")
        {
            std::path::Path::new(&home).join("terracotta")
        } else {
            std::path::Path::new(&env::temp_dir()).join("terracotta")
        };

        fs::create_dir_all(&path).unwrap();

        path
    };
    static ref MACHINE_ID_FILE: std::path::PathBuf = FILE_ROOT.join("machine-id");
    static ref WORKING_DIR: std::path::PathBuf = {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();

        (*FILE_ROOT).join(format!(
            "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{}",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            std::process::id()
        ))
    };
    static ref LOGGING_FILE: std::path::PathBuf = WORKING_DIR.join("application.log");
    static ref EASYTIER_DIR: std::path::PathBuf = WORKING_DIR.join("embedded-easytier");
}

#[derive(Debug, PartialEq)]
enum Mode {
    General,
    #[cfg(target_os = "macos")]
    Daemon,
    HMCL {
        file: String,
    },
}

#[rocket::main]
async fn main() {
    cfg_if::cfg_if! {
        if #[cfg(debug_assertions)] {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Short);
        } else {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Full);
        }
    }

    #[cfg(target_family = "windows")]
    {
        if unsafe { winapi::um::wincon::AttachConsole(u32::MAX) } != 0 {
            unsafe fn get_parent_id() -> u32 {
                use winapi::{
                    shared::minwindef::FALSE,
                    um::{
                        handleapi::CloseHandle,
                        tlhelp32::{
                            CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First,
                            Process32Next, TH32CS_SNAPPROCESS,
                        },
                    },
                };

                let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
                if snapshot.is_null() {
                    panic!("{:?}", std::io::Error::last_os_error());
                }
                let mut entry: PROCESSENTRY32 = unsafe { std::mem::zeroed() };
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

                if unsafe { Process32First(snapshot, &mut entry) } == FALSE {
                    unsafe { CloseHandle(snapshot) };
                    panic!("{:?}", std::io::Error::last_os_error());
                }

                let current_pid = std::process::id();
                loop {
                    if entry.th32ProcessID == current_pid {
                        return entry.th32ParentProcessID;
                    }
                    if unsafe { Process32Next(snapshot, &mut entry) } == FALSE {
                        break;
                    }
                }
                unsafe { CloseHandle(snapshot) };
                panic!("Cannot find parent process ID for PID {}", current_pid);
            }

            let parent = unsafe {
                winapi::um::processthreadsapi::OpenProcess(
                    winapi::um::winnt::SYNCHRONIZE,
                    winapi::shared::minwindef::FALSE,
                    get_parent_id(),
                )
            };
            if parent.is_null() {
                panic!("{:?}", std::io::Error::last_os_error());
            }

            let parent = std::sync::atomic::AtomicPtr::new(parent);
            thread::spawn(move || {
                let parent = parent.load(std::sync::atomic::Ordering::Acquire);

                unsafe {
                    use winapi::um::{synchapi::WaitForSingleObject, winbase::INFINITE};
                    WaitForSingleObject(parent, INFINITE);

                    winapi::um::wincon::FreeConsole();
                }
            });
        }
    }

    fn main_panic(arguments: Vec<String>) {
        logging!("UI", "Unknown arguments: {}", arguments.join(", "));
    }

    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.len() {
        0 => main_general(Mode::General).await,
        1 => match arguments[0].as_str() {
            #[cfg(target_os = "macos")]
            "--daemon" => main_daemon().await,
            "--help" => {
                println!("Welcoming using Terracotta | 陶瓦联机");
                println!("Usage: terracotta [OPTIONS]");
                println!("Options:");
                println!("  --help: Print this help message");
                println!("  --hmcl: [HMCL] For HMCL only.");
                #[cfg(target_os = "windows")]
                println!("  --hmcl2: [INTERNAL] For HMCL only.");
                #[cfg(target_os = "macos")]
                println!("  --daemon: [INTERNAL] Run in daemon mode.");
            }
            _ => main_panic(arguments),
        },
        2 => match arguments[0].as_str() {
            "--hmcl" => {
                cfg_if::cfg_if! {
                    if #[cfg(target_family = "windows")] {
                        use std::os::windows::process::CommandExt;
                        std::process::Command::new(std::env::current_exe().unwrap()).args(["--hmcl2", &arguments[1]]).creation_flags(0x08).spawn().unwrap();

                        let time = SystemTime::now();
                        while !SystemTime::now().duration_since(time).is_ok_and(|d| d > Duration::from_millis(8000)) {
                            if fs::File::open(&arguments[1]).is_ok() {
                                return;
                            }
                        }
                        panic!("Delegate process hasn't started in 8 seconds.");
                    } else {
                        main_general(Mode::HMCL {
                            file: arguments[1].clone(),
                        })
                        .await
                    }
                }
            }
            #[cfg(target_family = "windows")]
            "--hmcl2" => {
                main_general(Mode::HMCL {
                    file: arguments[1].clone(),
                })
                .await
            }
            _ => main_panic(arguments),
        },
        _ => main_panic(arguments),
    };
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        async fn main_daemon() {
            let state = Lock::get_state();
            match &state {
                Lock::Single { .. } => {
                    logging!("UI", "Running in daemon server mode.");
                    cleanup();

                    main_single(Some(state), Mode::Daemon).await;
                }
                Lock::Secondary { .. } => {
                    panic!("Deamon must run in server mode, but found secondary mode");
                }
                Lock::Unknown => {
                    panic!("Deamon must run in server mode, but found unknown mode");
                }
            };
        }

        async fn main_general(mode: Mode) {
            fn new_error<E>(error: E) -> Option<std::io::Error>
            where
                E: Into<Box<dyn std::error::Error + Send + Sync>>
            {
                return Some(std::io::Error::new(std::io::ErrorKind::TimedOut, error));
            }

            let state = Lock::get_state();
            let mut error = match &state {
                Lock::Single { .. } => {
                    drop(state);

                    match std::process::Command::new("launchctl")
                            .args([
                                "bootstrap",
                                &format!("gui/{}", unsafe { libc::getuid() }),
                                "/Library/LaunchAgents/net.burningtnt.terracotta.daemon.plist"
                            ])
                            .spawn()
                    {
                        Ok(mut process) => {
                            let start = SystemTime::now();
                            loop {
                                break match process.try_wait() {
                                    Ok(Some(status)) if status.success() => None,
                                    Ok(Some(status)) => new_error(format!("Process 'launchctl' failed: {:?}", status)),

                                    Ok(None) if SystemTime::now().duration_since(start).is_ok_and(|d| d >= Duration::from_secs(3)) =>
                                        new_error("Process 'launchctl' got stuck after 3s."),
                                    Ok(None) => continue,

                                    Err(e) => Some(e),
                                };
                            }
                        },
                        Err(e) => Some(e)
                    }
                },
                Lock::Secondary { port } => {
                    logging!("UI", "Running in secondary mode, port={}.", port);
                    main_secondary(*port, mode).await;
                    return;
                },
                Lock::Unknown => {
                    drop(state);
                    new_error("Cannot determin global lock state.")
                }
            };

            if let None = error {
                thread::sleep(Duration::from_millis(5000));

                let state = Lock::get_state();
                if let Lock::Secondary { port } = &state {
                    logging!("UI", "Running in secondary mode, port={}.", port);
                    main_secondary(*port, mode).await;
                    return;
                } else {
                    error = new_error("Cannot detect daemon process after 2000s.");
                }
            }

            if let Some(error) = error {
                if mode == Mode::General {
                    let _ = native_dialog::DialogBuilder::message()
                        .set_level(native_dialog::MessageLevel::Error)
                        .set_title("Terracotta | 陶瓦联机")
                        .set_text(format!("未能拉起后台守护进程，请尝试重启电脑，或与开发者联系。\n{}", error))
                        .alert()
                        .show();
                } else {
                    logging!("UI", "Failed to start daemon: {}", error);
                }
                return;
            }
        }
    } else {
        async fn main_general(mode: Mode) {
            cleanup();

            let state = Lock::get_state();
            match &state {
                Lock::Single { .. } => {
                    logging!("UI", "Running in server mode.");
                    main_single(Some(state), mode).await;
                },
                Lock::Secondary { port } => {
                    logging!("UI", "Running in secondary mode, port={}.", port);
                    cfg_if::cfg_if! {
                        if #[cfg(all(false, debug_assertions))] {
                            main_single(None, mode).await;
                        } else {
                            let port = *port;
                            drop(state);
                            main_secondary(port, mode).await;
                        }
                    }
                },
                Lock::Unknown => {
                    logging!("UI", "Running in unknown mode.");
                    main_single(None, mode).await;
                }
            }
        }
    }
}

async fn main_single(state: Option<Lock>, mode: Mode) {
    #[cfg(target_os = "macos")]
    assert!(matches!(mode, Mode::Daemon));

    redirect_std(&*LOGGING_FILE);

    let (port_callback, port_receiver) = mpsc::channel::<u16>();
    let port_callback2 = port_callback.clone();

    logging!(
        "UI", "Welcome using Terracotta v{}, compiled at {}. Easytier: {}. Target: {}-{}-{}-{}.",
        env!("TERRACOTTA_VERSION"),
        Utc.timestamp_millis_opt(timestamp::compile_time!() as i64).unwrap()
            .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M:%S"),
        env!("TERRACOTTA_ET_VERSION"),
        env!("CARGO_CFG_TARGET_ARCH"),
        env!("CARGO_CFG_TARGET_VENDOR"),
        env!("CARGO_CFG_TARGET_OS"),
        env!("CARGO_CFG_TARGET_ENV"),
    );

    let future = server::server_main(port_callback);
    thread::spawn(|| {
        lazy_static::initialize(&controller::SCAFFOLDING_PORT);
        lazy_static::initialize(&easytier::FACTORY);
    });

    thread::spawn(move || {
        let port = port_receiver.recv().unwrap();
        if port != 0 {
            if let Some(state) = state {
                state.set_port(port);
            }

            #[cfg(not(target_os = "macos"))]
            match mode {
                Mode::General => {
                    let _ = open::that(format!("http://127.0.0.1:{}/", port));
                }
                Mode::HMCL { file } => output_port(port, file),
            }
        }
    });

    future.await;
    let _ = port_callback2.send(0);

    easytier::FACTORY.remove();
}

async fn main_secondary(port: u16, mode: Mode) {
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(lock) = secondary_switch(port).await {
            logging!("UI", "Running in server mode.");
            main_single(Some(lock), mode).await;
            return;
        }
    }

    match mode {
        Mode::General => {
            cfg_if::cfg_if! {
                if #[cfg(target_os = "macos")] {
                    ui_macos::open(format!("http://127.0.0.1:{}/", port));
                } else {
                    let _ = open::that(format!("http://127.0.0.1:{}/", port));
                }
            }
        }
        #[cfg(target_os = "macos")]
        Mode::Daemon => assert!(false),
        Mode::HMCL { file } => output_port(port, file),
    }
}

async fn secondary_switch(port: u16) -> Option<Lock> {
    let client = reqwest::Client::new();

    let Ok(response) = client
        .get(format!("http://127.0.0.1:{}/meta", port))
        .send()
        .await
    else {
        return None;
    };
    let Ok(body) = response.text().await else {
        return None;
    };

    let Ok(value) = serde_json::from_str::<'_, serde_json::Value>(&body) else {
        return None;
    };
    let Some(compile_timestamp) = value.get("compile_timestamp").and_then(|v| v.as_str()) else {
        return None;
    };

    if let Ok(running) = compile_timestamp.parse::<u128>() && timestamp::compile_time!() > running
    {
        let Ok(response) = client
            .get(format!("http://127.0.0.1:{}/panic?peaceful=true", port))
            .send()
            .await
        else {
            return None;
        };

        if response.status().as_u16() == 502 {
            thread::sleep(Duration::from_millis(1500));
            let state = Lock::get_state();
            return match &state {
                Lock::Single { .. } => {
                    logging!("UI", "Running in server mode.");
                    Some(state)
                }
                Lock::Secondary { .. } | Lock::Unknown => None,
            };
        }
    }
    return None;
}

fn output_port(port: u16, file: String) {
    let mut f = fs::File::create(format!("{}.tmp", file)).unwrap();
    write!(f, "{}", serde_json::json!({"port": port})).unwrap();
    fs::rename(format!("{}.tmp", file), file).unwrap();
}

fn redirect_std(file: &'static std::path::PathBuf) {
    if cfg!(debug_assertions) {
        return;
    }

    let Some(parent) = file.parent() else {
        return;
    };

    if !fs::metadata(parent).is_ok() {
        if !fs::create_dir_all(parent).is_ok() {
            return;
        }
    }

    let Ok(logging_file) = fs::File::create(file.clone()) else {
        return;
    };

    logging!(
        "UI",
        "There will be not information on the console. Logs will be saved to {}",
        file.to_str().unwrap()
    );

    cfg_if::cfg_if! {
        if #[cfg(target_family = "unix")] {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::dup2(logging_file.as_raw_fd(), libc::STDOUT_FILENO);
                libc::dup2(logging_file.as_raw_fd(), libc::STDERR_FILENO);
            }

            std::mem::forget(logging_file);
        } else if #[cfg(target_family = "windows")] {
            logging_windows::redirect(logging_file);
        } else {
            compile_error!("Cannot redirect console on these platforms.");
        }
    }
}

fn cleanup() {
    thread::spawn(move || {
        let now = SystemTime::now();

        if let Ok(value) = fs::read_dir(&*FILE_ROOT) {
            for file in value {
                if let Ok(file) = file
                    && file
                        .path()
                        .file_name()
                        .and_then(|v| v.to_str())
                        .is_none_or(|v| v != "terracotta.lock")
                    && let Ok(metadata) = file.metadata()
                    && let Ok(file_type) = file.file_type()
                    && let Ok(time) = metadata.created()
                    && let Ok(duration) = now.duration_since(time)
                    && duration.as_secs()
                        >= if cfg!(debug_assertions) {
                            120
                        } else {
                            24 * 60 * 60
                        }
                    && let Err(e) = if file_type.is_dir() {
                        fs::remove_dir_all(file.path())
                    } else {
                        fs::remove_file(file.path())
                    }
                {
                    logging!("UI", "Cannot remove old file {:?}: {:?}", file.path(), e);
                }
            }
        }
    });
}
