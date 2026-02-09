use std::sync::mpsc;
use crate::controller::states::AppState;
use crate::controller::{scaffolding, ConnectionDifficulty, ExceptionType, Room};
use crate::scaffolding::profile::Profile;
use crate::mc::scanning::MinecraftScanner;
use crate::MOTD;
use rocket::serde::Serialize;
use serde::ser::SerializeSeq;
use serde::Serializer;
use serde_json::{json, Value};
use std::thread;
use std::time::{Duration, SystemTime};
use crate::easytier::publics::fetch_public_nodes;

pub fn get_state() -> Value {
    let state = AppState::acquire();
    let (index, sharing_index) = state.index();

    match state.as_ref() {
        AppState::Waiting => {
            json!({"state": "waiting", "index": index})
        }

        AppState::HostScanning { .. } => {
            json!({"state": "host-scanning", "index": index})
        }
        AppState::HostStarting { room, .. } => {
            json!({"state": "host-starting", "index": index, "room": room.code})
        }
        AppState::HostOk { room, profiles, .. } => {
            struct Holder<'a>(&'a Vec<(SystemTime, Profile)>);
            impl<'a> Serialize for Holder<'a> {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                    for (_, profile) in self.0 {
                        sequence.serialize_element(profile)?;
                    }
                    sequence.end()
                }
            }

            json!({"state": "host-ok", "index": index, "room": room.code, "profile_index": sharing_index, "profiles": Holder(profiles)})
        }

        AppState::GuestConnecting { room, .. } => {
            json!({"state": "guest-connecting", "index": index, "room": room.code})
        }
        AppState::GuestStarting { room, difficulty, .. } => {
            json!({"state": "guest-starting", "index": index, "room": room.code, "difficulty": match difficulty {
                ConnectionDifficulty::Unknown => "UNKNOWN",
                ConnectionDifficulty::Easiest => "EASIEST",
                ConnectionDifficulty::Simple => "SIMPLE",
                ConnectionDifficulty::Medium => "MEDIUM",
                ConnectionDifficulty::Tough => "TOUGH",
            }})
        }
        AppState::GuestOk { server, profiles, .. } => {
            let url = if server.port == 25565 {
                "127.0.0.1".into()
            } else {
                format!("127.0.0.1:{}", server.port)
            };

            json!({"state": "guest-ok", "index": index, "url": url, "profile_index": sharing_index, "profiles": profiles})
        }
        AppState::Exception { kind, .. } => json!({
            "state": "exception",
            "index": index,
            "type": match kind {
                ExceptionType::PingHostFail => 0,
                ExceptionType::PingHostRst => 1,
                ExceptionType::GuestEasytierCrash => 2,
                ExceptionType::HostEasytierCrash => 3,
                ExceptionType::PingServerRst => 4,
                ExceptionType::ScaffoldingInvalidResponse => 5,
            }
        }),
    }
}

pub fn set_waiting() {
    logging!("Core", "Setting to state WAITING.");

    let state = AppState::acquire();
    if matches!(state.as_ref(), AppState::Waiting) {
        return;
    }
    state.set(AppState::Waiting);
}

pub fn set_scanning(room: Option<String>, player: Option<String>, public_nodes: Vec<String>) {
    let capture = {
        let state = AppState::acquire();
        if !matches!(state.as_ref(), AppState::Waiting) {
            return;
        }

        state.set(AppState::HostScanning {
            scanner: MinecraftScanner::create(|m| m != MOTD),
        })
    };
    logging!("Core", "Setting to state SCANNING.");

    thread::spawn(move || {
        let room = room
            .and_then(|room| Room::from(&room))
            .unwrap_or_else(Room::create);

        let (sender, receiver) = mpsc::channel();
        let room2 = room.clone();
        thread::spawn(move || {
            // EasyTier Uptime is undergoing DDOS attack, so it's crucial to perform a prefetch logic.
            let _ = sender.send(fetch_public_nodes(&room2, public_nodes));
        });

        let (room, port, capture) = loop {
            thread::sleep(Duration::from_millis(200));

            let Some(state) = capture.try_capture() else {
                return;
            };
            let AppState::HostScanning { scanner, .. } = state.as_ref() else {
                unreachable!()
            };

            if let Some(port) = scanner.get_ports().first() {
                break (room.clone(), *port, state.set(AppState::HostStarting { room, port: *port }));
            }
        };

        scaffolding::start_host(room, port, player, capture, receiver.recv().unwrap())
    });
}

pub fn set_guesting(room: Room, player: Option<String>, public_nodes: Vec<String>) -> bool {
    let capture = {
        let state = AppState::acquire();
        if !matches!(state.as_ref(), AppState::Waiting { .. }) {
            return false;
        }
        state.set(AppState::GuestConnecting { room: room.clone() })
    };
    logging!("Core", "Connecting to room, code={}", room.code);
    thread::spawn(move || {
        let public_nodes = fetch_public_nodes(&room, public_nodes);
        scaffolding::start_guest(room, player, capture, public_nodes)
    });

    true
}
