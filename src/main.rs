#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        std::println!(
            "[{}]: {}",
            $prefix,
            std::format_args!($($arg)*)
        );
    };
}

#[macro_use]
extern crate rocket;
use lazy_static::lazy_static;

use std::{
    env, fs, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::mpsc,
    thread,
};

pub mod code;
pub mod easytier;
pub mod fakeserver;
pub mod scanning;
pub mod server;
pub mod time;

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
    static ref FILE_ROOT: std::path::PathBuf = if cfg!(target_os = "macos")
        && let Ok(home) = env::var("HOME")
    {
        std::path::Path::new(&home).join("terracotta")
    } else {
        std::path::Path::new(&env::temp_dir()).join("terracotta")
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
    thread::spawn(move || {
        let now = time::now();

        if let Ok(value) = fs::read_dir(&*FILE_ROOT) {
            for file in value {
                if let Ok(file) = file
                    && let Ok(metadata) = file.metadata()
                    && let Ok(file_type) = file.file_type()
                    && let Ok(time) = metadata.created()
                    && let Ok(duration) = now.duration_since(time)
                    && duration.as_secs()
                        >= if cfg!(debug_assertions) {
                            10
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

    fn wait<T>(obj: T) {
        let mut buf = String::from("");
        io::stdin().read_line(&mut buf).unwrap();
        std::mem::drop(obj);
    }

    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.len() {
        0 => main_auto().await,
        1 => match arguments[0].as_str() {
            "--auto" => main_auto().await,
            "--single" => main_single(None, false).await,
            "--daemon" => main_single(None, true).await,
            "--help" => {
                println!("Welcoming using Terracotta | 陶瓦联机");
                println!("Usage: terracotta [OPTIONS]");
                println!("Options:");
                println!("  --auto: Automatically determine the mode to run.");
                println!("  --single: Forcely run in single server mode.");
                println!("  --daemon: Forcely run in single server daemon mode.");
                println!("  --secondary <port>: Forcely run in secondary mode, opening an UI on the specified port.");
                println!("  --server <port>: Host a Terracotta Room on the specified port.");
                println!("  --client <room_code>: Join a Terracotta Room with the specified room code.");   
            },
            _ => main_panic(arguments),
        },
        2 => match arguments[0].as_str() {
            "--server" => {
                if let Ok(port) = arguments[1].parse::<u16>() {
                    let room = code::Room::create(port);
                    logging!(
                        "UI",
                        "Hosting Minecraft server, port = {}, room = {}.",
                        port,
                        room.code
                    );
                    wait(room.start());

                    easytier::FACTORY.drop_in_place();
                } else {
                    main_panic_msg(arguments, "Invalid room code");
                }
            },
            "--client" => {
                if let Ok(room) = code::Room::from(&arguments[1]) {
                    logging!(
                        "UI",
                        "Joining Minecraft server, port = {}, room = {}.",
                        room.port,
                        room.code
                    );
                    wait(room.start());

                    easytier::FACTORY.drop_in_place();
                } else {
                    main_panic_msg(arguments, "Invalid port number");
                }
            },
            "--secondary" => {
                if let Ok(port) = arguments[1].parse::<u16>() {
                    main_secondary(port);
                } else {
                    main_panic_msg(arguments, "Invalid port number");
                }
            },
            _ => main_panic(arguments),
        },
        _ => main_panic(arguments),
    };
}

fn main_panic(arguments: Vec<String>) {
    logging!("UI", "Unknown arguments: {}", arguments.join(", "));
}

fn main_panic_msg(arguments: Vec<String>, msg: &'static str) {
    logging!("UI", "{}: {}", msg, arguments.join(", "));
}

async fn main_auto() {
    let state = Lock::get_state();
    match &state {
        Lock::Single { .. } => {
            logging!("UI", "Running in server mode.");
            main_single(Some(state), false).await;
        }
        Lock::Secondary { port } => {
            logging!("UI", "Running in secondary mode, port={}.", port);

            main_secondary(*port);
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
    logging!("UI", "Running in secondary mode, port={}.", port);

    let _ = open::that(format!("http://127.0.0.1:{}/", port));
}

fn redirect_std(file: &'static std::path::PathBuf) {
    let is_enable = env::args().into_iter().any(|e| &e == "--redirect-std=yes");
    let is_disable = env::args().into_iter().any(|e| &e == "--redirect-std=no");

    if if is_enable != is_disable {
        is_disable
    } else {
        cfg!(debug_assertions)
    } {
        logging!("UI", "Log redirection is disabled.");
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

    #[cfg(target_family = "unix")]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::dup2(logging_file.as_raw_fd(), libc::STDOUT_FILENO);
            libc::dup2(logging_file.as_raw_fd(), libc::STDERR_FILENO);
        }
    }
    #[cfg(target_family = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let _ = winapi::um::processenv::SetStdHandle(
                winapi::um::winbase::STD_OUTPUT_HANDLE,
                logging_file.as_raw_handle() as _,
            );
            let _ = winapi::um::processenv::SetStdHandle(
                winapi::um::winbase::STD_ERROR_HANDLE,
                logging_file.as_raw_handle() as _,
            );
        }
    }

    Box::leak(Box::new(logging_file));
}
