use std::fmt::{Debug, Formatter};
use crate::easytier::EasyTier;
use crate::mc::fakeserver::FakeServer;
use crate::mc::scanning::MinecraftScanner;
use std::mem;
use std::panic::Location;
use std::time::{Duration, SystemTime};
use parking_lot::{Mutex, MutexGuard};
use crate::controller::Room;
use crate::scaffolding::profile::Profile;

pub enum AppState {
    Waiting,

    HostScanning {
        scanner: MinecraftScanner,
    },
    HostStarting {
        room: Room,
        port: u16,
    },
    HostOk {
        room: Room,
        port: u16,
        easytier: EasyTier,
        profiles: Vec<(SystemTime, Profile)>,
    },

    GuestConnecting {
        room: Room,
    },
    GuestStarting {
        room: Room,
        easytier: EasyTier,
    },
    GuestOk {
        room: Room,
        easytier: EasyTier,
        server: FakeServer,

        profiles: Vec<Profile>,
    },
    Exception {
        kind: ExceptionType,
    },
}

impl Debug for AppState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppState::Waiting => write!(f, "AppState::Waiting"),
            AppState::HostScanning { ..  } => {
                write!(f, "AppState::HostScanning {{ scanner: .. }}")
            }
            AppState::HostStarting { room, port } => {
                write!(f, "AppState::HostStarting {{ code: {:?}, port: {} }}", room.code, port)
            }
            AppState::HostOk { room, port, profiles, .. } => {
                write!(f, "AppState::HostOk {{ code: {:?}, port: {}, easytier: .., profiles: {:?} }}", room.code, port, profiles)
            }
            AppState::GuestConnecting { room } => {
                write!(f, "AppState::GuestConnecting {{ code: {:?} }}", room.code)
            }
            AppState::GuestStarting { room, .. } => {
                write!(f, "AppState::GuestStarting {{ code: {:?}, easytier: .. }}", room.code)
            }
            AppState::GuestOk { room, server, profiles, .. } => {
                write!(
                    f, "AppState::GuestOk {{ code: {:?}, server_port: {}, easytier: .., profiles: {:?} }}",
                    room.code, server.port, profiles
                )
            }
            AppState::Exception { kind } => {
                write!(f, "AppState::Exception {{ kind: {:?} }}", kind)
            }
        }
    }
}

#[derive(Debug)]
pub enum ExceptionType {
    PingHostFail,
    PingHostRst,
    GuestEasytierCrash,
    HostEasytierCrash,
    PingServerRst,
    ScaffoldingInvalidResponse,
}

pub struct AppStateContainer {
    state: MutexGuard<'static, Holder>,
    measure: Option<(SystemTime, &'static Location<'static>)>,
}

pub struct AppStateCapture {
    index: u32
}

struct Holder {
    index: u32,
    sharing: u32,
    value: AppState,
}

impl AppState {
    #[track_caller]
    pub fn acquire() -> AppStateContainer {
        static GLOBAL_STATE: Mutex<Holder> = Mutex::new(Holder { index: 0, sharing: 0, value: AppState::Waiting});

        AppStateContainer { state: GLOBAL_STATE.lock(), measure: Some((SystemTime::now(), Location::caller())) }
    }
}

impl AppStateContainer {
    pub fn into_slow(mut self) -> AppStateContainer {
        self.measure = None;
        self
    }

    pub fn as_ref(&self) -> &AppState {
        &self.state.value
    }

    pub fn as_mut_ref(&mut self) -> &mut AppState {
        &mut self.state.value
    }

    pub(crate) fn index(&self) -> (u32, u32) {
        (self.state.index, self.state.sharing)
    }

    pub fn set(mut self, state: AppState) -> AppStateCapture {
        self.state.value = state;
        self.increase()
    }

    pub fn replace<F>(mut self, f: F) -> AppStateCapture
    where
        F: FnOnce(AppState) -> AppState
    {
        let legacy = mem::replace(&mut self.state.value, AppState::Waiting);
        let _ = mem::replace(&mut self.state.value, f(legacy));

        self.increase()
    }

    pub fn increase(mut self) -> AppStateCapture {
        self.state.index += 1;
        self.state.sharing = 0;

        logging!("State", "Switch to {:?}", &self.state.value);
        AppStateCapture { index: self.state.index }
    }

    pub fn increase_shared(mut self) {
        self.state.index += 1;
        self.state.sharing += 1;

        logging!("State", "Switch (Shared) to {:?}", &self.state.value);
    }
}

impl Drop for AppStateContainer {
    fn drop(&mut self) {
        if let Some((time, location)) = self.measure &&
            let Ok(d) = SystemTime::now().duration_since(time) &&
            d >= Duration::from_millis(150)
        {
            cfg_if::cfg_if! {
                if #[cfg(debug_assertions)] {
                    panic!("AppState has been locked for {}ms, at {}.", d.as_millis(), location);
                } else {
                    logging!("State", "AppState has been locked for {}ms, at {}.", d.as_millis(), location);
                }
            }
        }
    }
}

impl AppStateCapture {
    #[track_caller]
    pub fn try_capture(&self) -> Option<AppStateContainer> {
        let container = AppState::acquire();
        let state = &container.state;
        if state.index - state.sharing <= self.index {
            Some(container)
        } else {
            None
        }
    }

    pub fn can_capture(&self) -> bool {
        self.try_capture().is_some()
    }
}