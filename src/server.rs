use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime};
use std::{mem, thread};

use rocket::http::Status;
use rocket::serde::json;
use socket2::{Domain, SockAddr, Socket, Type};

use crate::code::{self, Room};
use crate::easytier::Easytier;
use crate::fakeserver::FakeServer;
use crate::scanning::Scanning;
use crate::{LOGGING_FILE, time};

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
        server: FakeServer,
        ok: bool,
    },
    Exception {
        begin: SystemTime,
        kind: u8,
    },
}

const EXCEPTION_KIND_PING_HOST_FAIL: u8 = 0;
const EXCEPTION_KIND_PING_HOST_RST: u8 = 1;
const EXCEPTION_KIND_GUEST_ET_CRASH: u8 = 2;
const EXCEPTION_KIND_HOST_ET_CRASH: u8 = 3;

lazy_static::lazy_static! {
    static ref GLOBAL_STATE: Mutex<(u32, AppState)> = Mutex::new((
        0,
        AppState::Waiting {
            begin: time::now(),
        }
    ));
}

fn access_state() -> std::sync::MutexGuard<'static, (u32, AppState)> {
    let mut guard = GLOBAL_STATE.lock().unwrap();
    match &mut (*guard).1 {
        AppState::Waiting { begin } => {
            *begin = time::now();
        }
        AppState::Scanning { begin, .. } => {
            *begin = time::now();
        }
        AppState::Exception { begin, .. } => {
            *begin = time::now();
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
        static ref MAIN_PAGE: Vec<(PathBuf, Box<[u8]>)> = {
            let mut reader = sevenz_rust2::ArchiveReader::new(
                std::io::Cursor::new(WEB_STATIC),
                sevenz_rust2::Password::empty(),
            )
            .unwrap();
            let mut pages: Vec<(PathBuf, Box<[u8]>)> = vec![];
            let _ = reader.for_each_entries(|entry, reader| {
                if entry.is_directory() {
                    return Ok(true);
                }

                let mut buffer: Vec<u8> = vec![];
                reader.read_to_end(&mut buffer).unwrap();
                buffer.shrink_to_fit();
                pages.push((PathBuf::from(entry.name()), buffer.into_boxed_slice()));

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

            pages.shrink_to_fit();
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
        AppState::Guesting { server, ok, .. } => json::Json(json::json!({
            "state": "guesting",
            "index": v.0,
            "url": format!("127.0.0.1:{}", server.port),
            "ok": ok
        })),
        AppState::Exception { kind, .. } => json::Json(json::json!({
            "state": "exception",
            "index": v.0,
            "type": *kind
        })),
    };
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    logging!("UI", "Setting Server to state IDE.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Waiting { begin: time::now() };
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    logging!("UI", "Setting Server to state SCANNING.");

    let state = &mut *access_state();
    state.0 += 1;
    state.1 = AppState::Scanning {
        begin: time::now(),
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

        let (easytier, fake_server) = room.start();
        let fake_server = fake_server.unwrap();
        let port = fake_server.port;

        state.0 += 1;
        state.1 = AppState::Guesting {
            easytier: easytier,
            server: fake_server,
            ok: false,
        };

        let index = state.0;
        thread::spawn(move || {
            fn check_conn(port: u16) -> bool {
                let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(4)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(4)))
                    .unwrap();
                if let Ok(_) = socket.connect_timeout(&SockAddr::from(SocketAddrV4::new(
                    Ipv4Addr::new(127, 0, 0, 1),
                    port,
                )), Duration::from_secs(4)) {
                    if let Ok(_) = socket.send(&[0xFE]) {
                        let mut buf: [mem::MaybeUninit<u8>; 1] =
                            unsafe { mem::MaybeUninit::uninit().assume_init() };

                        if let Ok(size) = socket.recv(&mut buf)
                            && size >= 1
                            && unsafe { buf[0].assume_init() } == 0xFF
                        {
                            return true;
                        }
                    }
                }
                return false;
            }

            let mut ok = false;
            for _ in 0..5 {
                let end = time::now() + Duration::from_secs(5);
                if check_conn(port) {
                    ok = true;
                    break;
                }

                thread::sleep(end.duration_since(time::now()).unwrap_or(Duration::ZERO));
            }

            let index = {
                let mut state = access_state();
                if state.0 != index {
                    return;
                }

                state.0 += 1;
                if !ok {
                    state.1 = AppState::Exception {
                        begin: time::now(),
                        kind: EXCEPTION_KIND_PING_HOST_FAIL,
                    };

                    return;
                }

                if let AppState::Guesting { ok, .. } = &mut state.1 {
                    *ok = true;
                } else {
                    panic!("State has been changed without increasing index.");
                }

                logging!("UI", "Room is ready, port = {}.", port);
                state.0
            };

            let mut error_count: u8 = 0;
            loop {
                let end = time::now() + Duration::from_secs(5);
                if check_conn(port) {
                    error_count = 0;
                } else {
                    error_count += 1;
                    if error_count >= 5 {
                        let mut state = access_state();
                        if state.0 != index {
                            return;
                        }

                        logging!("UI", "Connection to room has been lost, port = {}.", port);
                        state.0 += 1;
                        state.1 = AppState::Exception {
                            begin: time::now(),
                            kind: EXCEPTION_KIND_PING_HOST_RST,
                        };
                        return;
                    }
                }

                if access_state().0 != index {
                    return;
                }

                thread::sleep(end.duration_since(time::now()).unwrap_or(Duration::ZERO));
            }
        });
        return Status::Ok;
    }

    return Status::BadRequest;
}

#[get("/log")]
fn download_log() -> std::fs::File {
    return std::fs::File::open((*LOGGING_FILE).clone()).unwrap();
}

#[get("/meta")]
fn get_meta() -> json::Json<json::Value> {
    return json::Json(json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "easytier_version": env!("TERRACOTTA_ET_VERSION"),
        "target_tuple": format!(
            "{}-{}-{}-{}",
            env!("CARGO_CFG_TARGET_ARCH"),
            env!("CARGO_CFG_TARGET_VENDOR"),
            env!("CARGO_CFG_TARGET_OS"),
            env!("CARGO_CFG_TARGET_ENV"),
         ),
        "target_arch": env!("CARGO_CFG_TARGET_ARCH"),
        "target_vendor": env!("CARGO_CFG_TARGET_VENDOR"),
        "target_os": env!("CARGO_CFG_TARGET_OS"),
        "target_env": env!("CARGO_CFG_TARGET_ENV"),
    }));
}

pub async fn server_main(port: mpsc::Sender<u16>, daemon: bool) {
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
            get_meta,
        ],
    )
    .attach(rocket::fairing::AdHoc::on_liftoff(
        "Open Browser",
        move |rocket| {
            Box::pin(async move {
                launch_signal_tx.send(()).unwrap();

                let local_port = rocket.config().port;
                if !cfg!(debug_assertions) && !daemon {
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
                if cfg!(target_os = "macos") {
                    return false;
                }

                const TIMEOUT: u64 = if cfg!(debug_assertions) { 20 } else { 600 };

                if let Ok(timeout) = time::now().duration_since(*time) {
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

            if let Ok(_) = launch_signal_rx.try_recv() {
                return;
            }

            let mut state = GLOBAL_STATE.lock().unwrap();
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
                        state.1 = AppState::Exception {
                            begin: time::now(),
                            kind: EXCEPTION_KIND_HOST_ET_CRASH,
                        };
                    }
                }
                AppState::Guesting { easytier, .. } => {
                    if !easytier.is_alive() {
                        logging!("UI", "Easytier has been dead.");
                        state.0 += 1;
                        state.1 = AppState::Exception {
                            begin: time::now(),
                            kind: EXCEPTION_KIND_GUEST_ET_CRASH,
                        };
                    }
                }
                AppState::Exception { begin, .. } => {
                    if handle_offline(begin) {
                        shutdown.notify();
                        return;
                    }
                }
            };

            drop(state);
            thread::sleep(Duration::from_millis(200));
        }
    });

    let _ = rocket.launch().await;
    let _ = shutdown_signal_tx.send(());
}
