use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rocket::http::Status;
use rocket::response::content::RawHtml;
use rocket::serde::json;

use crate::fakeserver::FakeServer;
use crate::scanning::Scanning;
use crate::easytier::Easytier;
use crate::code::{self, Room};
use crate::LOGGING_FILE;

enum AppState {
    Waiting {
        begin: Instant,
    },
    Scanning {
        begin: Instant,
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
            begin: Instant::now(),
        }
    ));
}

fn access_state() -> std::sync::MutexGuard<'static, (u32, AppState)> {
    let mut guard = GLOBAL_STATE.lock().unwrap();
    match &mut (*guard).1 {
        AppState::Waiting { begin } => {
            *begin = Instant::now();
        }
        AppState::Scanning { begin, .. } => {
            *begin = Instant::now();
        }
        _ => {}
    }

    return guard;
}

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webstatics.7z"));

#[get("/")]
fn index() -> Result<RawHtml<&'static str>, Status> {
    lazy_static::lazy_static! {
        static ref MAIN_PAGE: String = {
            let mut reader = sevenz_rust2::ArchiveReader::new(
                std::io::Cursor::new(WEB_STATIC),
                sevenz_rust2::Password::empty(),
            )
            .unwrap();

            String::from_utf8(reader.read_file("_.html").unwrap()).unwrap()
        };
    }

    return Ok(RawHtml(&MAIN_PAGE));
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
        begin: Instant::now(),
    };
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    logging!("UI", "Setting Server to state SCANNING.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Scanning {
        begin: Instant::now(),
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
        index,
        get_state,
        set_state_ide,
        set_state_scanning,
        set_state_guesting,
        download_log
    ]
    )
    .attach(rocket::fairing::AdHoc::on_liftoff("Open Browser", move |rocket| {
        Box::pin(async move {
            launch_signal_tx.send(()).unwrap();
            
            let local_port = rocket.config().port;
            let _ = open::that(format!("http://127.0.0.1:{}/", local_port));
            let _ = port.send(local_port);
        })
    }))
    .ignite()
    .await
    .unwrap();

    let shutdown: rocket::Shutdown = rocket.shutdown();
    std::thread::spawn(move || {
        launch_signal_rx.recv().unwrap();

        loop {
            fn handle_offline(time: &Instant) -> bool {
                const TIMEOUT: u128 = if cfg!(debug_assertions) { 3000 } else { 10000 };

                let timeout = Instant::now().duration_since(*time).as_millis();
                if timeout >= TIMEOUT {
                    logging!(
                        "UI",
                        "Server has been in IDE state for {}s. Shutting down.",
                        TIMEOUT / 1000
                    );
                    return true;
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
                            begin: Instant::now(),
                        };
                    }
                }
                AppState::Guesting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Waiting {
                            begin: Instant::now(),
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