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

use std::{env, process, sync::mpsc, thread::spawn};

pub mod code;
pub mod easytier;
pub mod fakeserver;
pub mod scanning;
pub mod server;

#[cfg(windows)]
pub mod lock_windows;
#[cfg(windows)]
use lock_windows::State as lock;
#[cfg(unix)]
pub mod lock_unix;
#[cfg(unix)]
use lock_unix::State as lock;

lazy_static! {
    static ref LOGGING_FILE: std::path::PathBuf = std::path::Path::join(
        &env::temp_dir(),
        format!("terracotta-rs-logging-{}.log", process::id()),
    );
}

#[rocket::main]
async fn main() {
    let state = lock::get_state();
    match &state {
        lock::Single { .. } => {
            logging!("UI", "Running in server mode.");

            let (tx, rx) = mpsc::channel::<u16>();
            let tx2 = tx.clone();

            let future = main_server(tx);
            spawn(move || {
                let port = rx.recv().unwrap();
                if port != 0 {
                    state.set_port(port);
                }
            });

            future.await;
            let _ = tx2.send(0);
        }
        lock::Secondary { port } => {
            logging!("UI", "Running in secondary mode, port={}.", port);

            let _ = open::that(format!("http://127.0.0.1:{}/", port));
        }
        lock::Unknown => {
            logging!("UI", "Cannot determin application mode. Fallback to server mode.");

            let (tx, _) = mpsc::channel::<u16>();
            main_server(tx).await;
        }
    };
}

async fn main_server(port: mpsc::Sender<u16>) {
    logging!("UI", "Logs will be saved to {}. There will be not information on the console.", (*LOGGING_FILE).to_str().unwrap());

    let logging_file = std::fs::File::create((*LOGGING_FILE).clone()).unwrap();
    if cfg!(not(debug_assertions)) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::dup2(logging_file.as_raw_fd(), libc::STDOUT_FILENO);
                libc::dup2(logging_file.as_raw_fd(), libc::STDERR_FILENO);
            }
        }
        #[cfg(windows)]
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
    }

    server::server_main(port).await;
}
