use std::path::PathBuf;
use std::sync::{Arc, RwLock, mpsc};
use std::thread;
use std::time::Duration;

use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{Value, json};

use crate::code::Room;
use crate::{LOGGING_FILE, core};

static WEB_STATIC: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webstatics.7z"));

struct MemoryFile(Arc<Storage>);
struct Storage {
    path: PathBuf,
    data: Box<[u8]>,
}

impl AsRef<[u8]> for MemoryFile {
    fn as_ref(&self) -> &[u8] {
        return self.0.as_ref().data.as_ref();
    }
}

impl<'r> rocket::response::Responder<'r, 'static> for MemoryFile {
    fn respond_to(self, _: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        use rocket::http::ContentType;
        use std::io::Cursor;

        let ct = self
            .0
            .as_ref()
            .path
            .extension()
            .and_then(|ext| ContentType::from_extension(&ext.to_string_lossy()));

        let mut response = rocket::Response::build()
            .header(ContentType::Binary)
            .sized_body(self.0.as_ref().data.len(), Cursor::new(self))
            .ok()?;

        if let Some(ct) = ct {
            response.set_header(ct);
        }

        Ok(response)
    }
}

#[get("/<path..>")]
fn static_files(path: PathBuf) -> Result<MemoryFile, Status> {
    fn compute_static_pages() -> Vec<Arc<Storage>> {
        let mut reader = sevenz_rust2::ArchiveReader::new(
            std::io::Cursor::new(WEB_STATIC),
            sevenz_rust2::Password::empty(),
        )
        .unwrap();
        let mut pages: Vec<Arc<Storage>> = vec![];
        let _ = reader.for_each_entries(|entry, reader| {
            if entry.is_directory() {
                return Ok(true);
            }

            let mut buffer: Vec<u8> = vec![];
            reader.read_to_end(&mut buffer).unwrap();
            pages.push(Arc::new(Storage {
                path: PathBuf::from(entry.name()),
                data: buffer.into_boxed_slice(),
            }));

            return Ok(true);
        });

        pages.shrink_to_fit();
        return pages;
    }

    use std::sync::mpsc::{self, Sender};
    lazy_static::lazy_static! {
        static ref MAIN_PAGE: RwLock<Option<(Sender<()>, Vec<Arc<Storage>>)>> = RwLock::new(None);
    }

    fn respond(mut path: PathBuf, storages: &Vec<Arc<Storage>>) -> Result<MemoryFile, Status> {
        if path.as_os_str().is_empty() {
            path = PathBuf::from("_.html");
        }
        return match storages.iter().find(|storage| path == storage.path) {
            Some(storage) => Ok(MemoryFile(storage.clone())),
            None => Err(Status { code: 404 }),
        };
    }

    let lock = MAIN_PAGE.read().unwrap();
    match lock.as_ref() {
        Some((sender, storages)) => {
            let _ = sender.send(());
            return respond(path, storages);
        }
        None => {
            drop(lock);

            let mut lock = MAIN_PAGE.write().unwrap();
            match lock.as_ref() {
                Some((sender, storages)) => {
                    let _ = sender.send(());
                    return respond(path, storages);
                }
                None => {
                    let pages = compute_static_pages();
                    let respond = respond(path, &pages);

                    let (sender, receiver) = mpsc::channel();
                    thread::spawn(move || {
                        loop {
                            if let Err(_) = receiver.recv_timeout(Duration::from_secs(60)) {
                                let mut lock = MAIN_PAGE.write().unwrap();
                                logging!(
                                    "UI",
                                    "Invaliding static page cache to reduce memory usage."
                                );
                                *lock = None;
                                return;
                            }
                        }
                    });

                    *lock = Some((sender, pages));
                    return respond;
                }
            }
        }
    }
}

#[get("/state")]
fn get_state() -> Json<Value> {
    return Json(core::get_state());
}

#[get("/state/ide")]
fn set_state_ide() -> Status {
    core::set_waiting();
    return Status::Ok;
}

#[get("/state/scanning")]
fn set_state_scanning() -> Status {
    core::set_scanning();
    return Status::Ok;
}

#[get("/state/guesting?<room>")]
fn set_state_guesting(room: Option<String>) -> Status {
    if let Some(room) = room
        && let Ok(room) = Room::from(&room)
    {
        core::set_guesting(room);
        return Status::Ok;
    }

    return Status::BadRequest;
}

cfg_if::cfg_if! {
    if #[cfg(target_os = "macos")] {
        #[get("/log")]
        fn download_log() -> Status {
            use std::process::Command;
            return match Command::new("open")
                .arg((*LOGGING_FILE).parent().unwrap())
                .spawn() {
                    Ok(_) => Status::Ok,
                    Err(e) => {
                        logging!("Core", "Cannot open logging file: {:?}", e);
                        Status::InternalServerError
                    }
                };
        }
    } else {
        #[get("/log")]
        fn download_log() -> std::fs::File {
            return std::fs::File::open((*LOGGING_FILE).clone()).unwrap();
        }
    }
}

#[get("/panic")]
fn panic() {
    panic!();
}

#[get("/meta")]
fn get_meta() -> Json<Value> {
    return Json(json!({
        "version": env!("TERRACOTTA_VERSION"),
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

pub async fn server_main(port_callback: mpsc::Sender<u16>, daemon: bool) {
    core::ExceptionType::register_hook(|_| {
        // TODO: Send system notifications.
    });

    let _ = rocket::custom(rocket::Config {
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
            panic,
        ],
    )
    .attach(rocket::fairing::AdHoc::on_liftoff(
        "Open Browser",
        move |rocket| {
            Box::pin(async move {
                let port = rocket.config().port;
                if !cfg!(debug_assertions) && !daemon {
                    let _ = open::that(format!("http://127.0.0.1:{}/", port));
                }
                let _ = port_callback.send(port);

                if !daemon {
                    let shutdown = rocket.shutdown();
                    thread::spawn(move || {
                        loop {
                            if let Some(duration) = core::get_waiting_time()
                                && duration > Duration::from_secs(600)
                            {
                                shutdown.notify();
                                return;
                            }

                            thread::sleep(Duration::from_millis(200));
                        }
                    });
                }
            })
        },
    ))
    .launch()
    .await;
}
