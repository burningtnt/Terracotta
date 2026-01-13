pub mod scaffolding;

use crate::controller::states::AppStateCapture;

#[derive(Debug, Clone)]
pub struct Room {
    pub code: String,

    pub network_name: String,
    pub network_secret: String,
    #[allow(dead_code)]
    pub kind: RoomKind,
}

#[derive(Debug, Clone)]
pub enum RoomKind {
    Scaffolding { #[allow(dead_code)] seed: u128 }
}

#[derive(Debug)]
pub enum ConnectionDifficulty {
    Unknown, Easiest, Simple, Medium, Tough
}

impl Room {
    pub fn create() -> Room {
        scaffolding::create_room()
    }

    pub fn from(code: &str) -> Option<Room> {
        scaffolding::parse(code)
    }

    pub fn start_guest(self, capture: AppStateCapture, player: Option<String>) {
        scaffolding::start_guest(self, player, capture)
    }
}
