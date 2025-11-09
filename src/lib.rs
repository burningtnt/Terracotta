#![feature(panic_backtrace_config, const_convert, const_trait_impl, unsafe_cell_access)]

extern crate core;
#[macro_use]
extern crate rocket;

#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        std::println!("[{}]: {}", $prefix, std::format_args!($($arg)*));
    };
}

use lazy_static::lazy_static;

use chrono::{FixedOffset, TimeZone, Utc};
use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::mpsc,
    thread,
    time::SystemTime,
};
use jni_sys::{jint, jobject, JNIEnv};

pub mod controller;
pub mod easytier;
pub mod scaffolding;
pub mod server;
pub const MOTD: &'static str = "§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）";

mod mc;
mod ports;

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
}

#[no_mangle]
pub extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_start(_env: *mut JNIEnv) -> jint {
    run() as jint
}

fn run() -> i16 {
    cfg_if::cfg_if! {
        if #[cfg(debug_assertions)] {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Short);
        } else {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Full);
        }
    }

    cleanup();
    redirect_std(&*LOGGING_FILE);

    let (port_callback, port_receiver) = mpsc::channel::<u16>();
    let port_callback2 = port_callback.clone();

    logging!(
        "UI",
        "Welcome using Terracotta v{}, compiled at {}. Easytier: {}. Target: {}-{}-{}-{}.",
        env!("TERRACOTTA_VERSION"),
        Utc.timestamp_millis_opt(timestamp::compile_time!() as i64)
            .unwrap()
            .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M:%S"),
        env!("TERRACOTTA_ET_VERSION"),
        env!("CARGO_CFG_TARGET_ARCH"),
        env!("CARGO_CFG_TARGET_VENDOR"),
        env!("CARGO_CFG_TARGET_OS"),
        env!("CARGO_CFG_TARGET_ENV"),
    );

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .spawn(server::server_main(port_callback));

    thread::spawn(|| {
        lazy_static::initialize(&controller::SCAFFOLDING_PORT);
        lazy_static::initialize(&easytier::FACTORY);
    });

    return match port_receiver.recv() {
        Ok(port) => port as i16,
        Err(_) => -1,
    };
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

    use std::os::unix::io::AsRawFd;
    unsafe {
        libc::dup2(logging_file.as_raw_fd(), libc::STDOUT_FILENO);
        libc::dup2(logging_file.as_raw_fd(), libc::STDERR_FILENO);
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
