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
                crate::logging::logging($prefix, std::format_args!($($arg)*));
            } else {
                std::println!("[{}]: {}", $prefix, std::format_args!($($arg)*));
            }
        };

    };
}

#[macro_use]
extern crate rocket;
use lazy_static::lazy_static;

use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

pub mod code;
pub mod core;
pub mod easytier;
pub mod fakeserver;
#[cfg(target_family = "windows")]
pub mod logging;
pub mod scanning;
pub mod server;

#[cfg(target_os = "macos")]
pub mod ui_macos;

pub const MOTD: &'static str = "§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）";

#[cfg(target_family = "windows")]
pub mod lock_windows;
#[cfg(target_family = "windows")]
use lock_windows::State as Lock;
#[cfg(target_family = "unix")]
pub mod lock_unix;
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

#[rocket::main]
async fn main() {
    std::panic::set_backtrace_style(std::panic::BacktraceStyle::Full);

    #[cfg(target_family = "windows")]
    {
        if unsafe { winapi::um::wincon::AttachConsole(u32::MAX) } == 0
            && std::io::Error::last_os_error().raw_os_error().unwrap() != 0x6
        {
            if unsafe { winapi::um::consoleapi::AllocConsole() } == 0 {
                panic!("{:?}", std::io::Error::last_os_error());
            }
        }

        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};

        if let Ok(f) = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(false)
            .share_mode(0x2)
            .open("CONOUT$")
        {
            let handle: winapi::um::winnt::HANDLE = f.as_raw_handle() as _;
            std::mem::forget(f);

            unsafe {
                for h in [
                    winapi::um::winbase::STD_OUTPUT_HANDLE,
                    winapi::um::winbase::STD_ERROR_HANDLE,
                ] {
                    if winapi::um::processenv::SetStdHandle(h, handle) == 0 {
                        panic!("{:?}", std::io::Error::last_os_error());
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    cleanup();

    fn main_panic(arguments: Vec<String>) {
        logging!("UI", "Unknown arguments: {}", arguments.join(", "));
    }

    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.len() {
        0 => main_general().await,
        1 => match arguments[0].as_str() {
            "--daemon" => main_daemon().await,
            "--help" => {
                println!("Welcoming using Terracotta | 陶瓦联机");
                println!("Usage: terracotta [OPTIONS]");
                println!("Options:");
                println!("  --daemon: Run in daemon mode.");
            }
            _ => main_panic(arguments),
        },
        _ => main_panic(arguments),
    };
}

async fn main_general() {
    let state = Lock::get_state();
    match &state {
        Lock::Single { .. } => {
            logging!("UI", "Running in server mode.");

            cfg_if::cfg_if! {
                if #[cfg(target_os = "macos")] {
                    let _ = native_dialog::DialogBuilder::message()
                        .set_level(native_dialog::MessageLevel::Error)
                        .set_title("Terracotta | 陶瓦联机")
                        .set_text("未检测到后台守护进程，请尝试重启电脑，或与开发者联系。")
                        .alert()
                        .show();
                } else {
                    main_single(Some(state), false).await;
                }
            }
        }
        Lock::Secondary { port } => {
            logging!("UI", "Running in secondary mode, port={}.", port);

            main_secondary(*port);

            if cfg!(target_os = "macos") {
                thread::sleep(Duration::from_secs(5));
            }
        }
        Lock::Unknown => {
            logging!(
                "UI",
                "Cannot determin application mode. Fallback to server mode."
            );

            main_single(None, false).await;
        }
    };
}

async fn main_daemon() {
    let state = Lock::get_state();
    match &state {
        Lock::Single { .. } => {
            logging!("UI", "Running in daemon server mode.");
            #[cfg(target_os = "macos")]
            cleanup();

            main_single(Some(state), true).await;
        }
        Lock::Secondary { port } => {
            logging!("UI", "Running in daemon secondary mode, port={}.", port);
        }
        Lock::Unknown => {
            logging!(
                "UI",
                "Cannot determin application mode. Fallback to server mode."
            );

            main_single(None, true).await;
        }
    };
}

async fn main_single(state: Option<Lock>, daemon: bool) {
    redirect_std(&*LOGGING_FILE);

    let (tx, rx) = mpsc::channel::<u16>();
    let tx2 = tx.clone();

    let future = server::server_main(tx, daemon);
    thread::spawn(|| {
        let _ = &*easytier::FACTORY;
    });

    if let Some(state) = state {
        thread::spawn(move || {
            let port = rx.recv().unwrap();
            if port != 0 {
                state.set_port(port);
            }
        });
    }

    future.await;
    let _ = tx2.send(0);

    easytier::FACTORY.drop_in_place();
}

fn main_secondary(port: u16) {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            ui_macos::open(format!("http://127.0.0.1:{}/", port));
        } else {
            let _ = open::that(format!("http://127.0.0.1:{}/", port));
        }
    }
    
    output_port(port);
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
            logging::redirect(logging_file);
        } else {
            compile_error!("Cannot redirect console on these platforms.");
        }
    }
}

fn output_port(port: u16) {
    logging!(
        ":",
        "{}",
        serde_json::json!({
            "version": 1,
            "url": format!("http://127.0.0.1:{}/", port)
        })
    );
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
                            2
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