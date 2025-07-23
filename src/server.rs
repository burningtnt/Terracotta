use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use rocket::http::Status;
use rocket::serde::json;

use crate::LOGGING_FILE;
use crate::code::{self, Room};
use crate::easytier::Easytier;
use crate::fakeserver::FakeServer;
use crate::scanning::Scanning;

enum AppState {
    Waiting {
        begin: SystemTime,
    },
    Scanning {
        begin: SystemTime,
        scanner: Scanning,
    },
    Hosting {
        easytier: Easytier,
        room: Room,
    },
    Guesting {
        easytier: Easytier,
        _entry: FakeServer,
        _room: Room,
    },
}

lazy_static::lazy_static! {
    static ref GLOBAL_STATE: Mutex<(u32, AppState)> = Mutex::new((
        0,
        AppState::Waiting {
            begin: SystemTime::now(),
        }
    ));
}

fn access_state() -> std::sync::MutexGuard<'static, (u32, AppState)> {
    let mut guard = GLOBAL_STATE.lock().unwrap();
    match &mut (*guard).1 {
        AppState::Waiting { begin } => {
            *begin = SystemTime::now();
        }
        AppState::Scanning { begin, .. } => {
            *begin = SystemTime::now();
        }
        _ => {}
    }

    return guard;
}

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webstatics.7z"));

pub struct MemoryFile(PathBuf, &'static [u8]);

impl<'r> rocket::response::Responder<'r, 'static> for MemoryFile {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        let mut response = self.1.respond_to(req)?;
        if let Some(ext) = self.0.extension() {
            if let Some(ct) = rocket::http::ContentType::from_extension(&ext.to_string_lossy()) {
                response.set_header(ct);
            }
        }

        Ok(response)
    }
}

#[get("/<path..>")]
fn static_files(mut path: PathBuf) -> Result<MemoryFile, Status> {
    lazy_static::lazy_static! {
        static ref MAIN_PAGE: Vec<(PathBuf, &'static [u8])> = {
            let mut reader = sevenz_rust2::ArchiveReader::new(
                std::io::Cursor::new(WEB_STATIC),
                sevenz_rust2::Password::empty(),
            )
            .unwrap();
            let mut pages: Vec<(PathBuf, &'static [u8])> = vec![];
            let _ = reader.for_each_entries(|entry, reader| {
                if entry.is_directory() {
                    return Ok(true);
                }

                let mut buffer: Vec<u8> = vec![];
                reader.read_to_end(&mut buffer).unwrap();
                pages.push((PathBuf::from(entry.name()), Box::leak(Box::new(buffer))));

                return Ok(true);
            });

            #[cfg(debug_assertions)] {
                let mut msg = String::from("Loading static files: ");
                for (path, data) in pages.iter() {
                    msg.push_str("\n- ");
                    msg.push_str(path.as_os_str().to_str().unwrap());
                    msg.push_str(": ");
                    msg.push_str(&data.len().to_string());
                    msg.push_str(" bytes");
                }
                logging!("UI", "{}", msg);
            }

            pages
        };
    }

    if path.as_os_str().is_empty() {
        path = PathBuf::from("_.html");
    }

    return match MAIN_PAGE.iter().find(|(entry, _)| *entry == path) {
        Some((_, data)) => Ok(MemoryFile(path, data)),
        None => Err(Status { code: 404 }),
    };
}

#[get("/state")]
fn get_state() -> json::Json<json::Value> {
    let v = &mut *access_state();
    return match &v.1 {
        AppState::Waiting { .. } => json::Json(json::json!({"state": "waiting", "index": v.0})),
        AppState::Scanning { .. } => json::Json(json::json!({"state": "scanning", "index": v.0})),
        AppState::Hosting { room, .. } => json::Json(json::json!({
            "state": "hosting",
            "index": v.0,
            "room": room.code
        })),
        AppState::Guesting { .. } => json::Json(json::json!({
            "state": "guesting",
            "index": v.0,
            "url": format!("127.0.0.1:{}", code::LOCAL_PORT)
        })),
    };
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    logging!("UI", "Setting Server to state IDE.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Waiting {
        begin: SystemTime::now(),
    };
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    logging!("UI", "Setting Server to state SCANNING.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Scanning {
        begin: SystemTime::now(),
        scanner: Scanning::create(|motd| motd != code::MOTD),
    };
    return Status::Ok;
}

#[get("/state/guesting?<room>")]
fn set_state_guesting(room: Option<String>) -> Status {
    if let Some(room) = room
        && let Ok(room) = Room::from(&room)
    {
        logging!(
            "UI",
            "Setting Server to state GUESTING, room = {}.",
            room.code
        );

        let state = &mut *access_state();
        state.0 += 1;
        let (easytier, entry) = room.start();
        state.1 = AppState::Guesting {
            easytier: easytier,
            _entry: entry.unwrap(),
            _room: room,
        };
        return Status::Ok;
    }

    return Status::BadRequest;
}

#[get("/log")]
fn download_log() -> std::fs::File {
    return std::fs::File::open((*LOGGING_FILE).clone()).unwrap();
}

pub async fn server_main(port: mpsc::Sender<u16>) {
    let (launch_signal_tx, launch_signal_rx) = mpsc::channel::<()>();
    let shutdown_signal_tx = launch_signal_tx.clone();

    let rocket = rocket::custom(rocket::Config {
        log_level: rocket::log::LogLevel::Critical,
        port: if cfg!(debug_assertions) { 8080 } else { 0 },
        ..rocket::Config::default()
    })
    .mount(
        "/",
        routes![
            get_state,
            set_state_ide,
            set_state_scanning,
            set_state_guesting,
            download_log,
            static_files,
        ],
    )
    .attach(rocket::fairing::AdHoc::on_liftoff(
        "Open Browser",
        move |rocket| {
            Box::pin(async move {
                launch_signal_tx.send(()).unwrap();

                let local_port = rocket.config().port;
                if !cfg!(debug_assertions) {
                    let _ = open::that(format!("http://127.0.0.1:{}/", local_port));
                }
                let _ = port.send(local_port);
            })
        },
    ))
    .ignite()
    .await
    .unwrap();

    let shutdown: rocket::Shutdown = rocket.shutdown();
    std::thread::spawn(move || {
        launch_signal_rx.recv().unwrap();

        loop {
            fn handle_offline(time: &SystemTime) -> bool {
                const TIMEOUT: u64 = if cfg!(debug_assertions) { 20 } else { 600 };

                if let Ok(timeout) = SystemTime::now().duration_since(*time) {
                    let timeout = timeout.as_secs();
                    if timeout >= TIMEOUT {
                        logging!(
                            "UI",
                            "Server has been in IDE state for {}s. Shutting down.",
                            TIMEOUT
                        );
                        return true;
                    }
                }
                return false;
            }

            if let Ok(_) = launch_signal_rx.recv_timeout(Duration::from_millis(200)) {
                return;
            }

            let state = &mut *GLOBAL_STATE.lock().unwrap();
            match &mut state.1 {
                AppState::Waiting { begin } => {
                    if handle_offline(begin) {
                        shutdown.notify();
                        return;
                    }
                }
                AppState::Scanning { begin, scanner } => {
                    if handle_offline(begin) {
                        shutdown.notify();
                        return;
                    }

                    let ports = scanner.get_ports();
                    if let Some(port) = ports.get(0) {
                        let room = Room::create(*port);
                        logging!(
                            "UI",
                            "Setting Server to state HOSTING, port = {}, room = {}.",
                            port,
                            room.code
                        );

                        state.0 += 1;
                        state.1 = AppState::Hosting {
                            easytier: room.start().0,
                            room: room,
                        };
                    }
                }
                AppState::Hosting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Waiting {
                            begin: SystemTime::now(),
                        };
                    }
                }
                AppState::Guesting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Waiting {
                            begin: SystemTime::now(),
                        };
                    }
                }
            };

            thread::sleep(Duration::from_millis(200));
        }
    });

    let _ = rocket.launch().await;
    let _ = shutdown_signal_tx.send(());
}
