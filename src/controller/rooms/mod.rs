pub mod experimental;
mod legacy;
mod pcl2ce;

use crate::controller::states::AppStateCapture;

#[derive(Debug, Clone)]
pub struct Room {
    pub code: String,

    pub(crate) network_name: String,
    pub(crate) network_secret: String,
    pub(crate) kind: RoomKind,
}

#[derive(Debug, Clone)]
pub(crate) enum RoomKind {
    Experimental { seed: u128 },
    TerracottaLegacy { mc_port: u16 },
    PCL2CE { mc_port: u16 },
}

impl Room {
    pub fn create() -> Room {
        experimental::create_room()
    }

    pub fn from(code: &str) -> Option<Room> {
        for parser in [experimental::parse, legacy::parse, pcl2ce::parse] {
            if let Some(room) = parser(code) {
                return Some(room);
            }
        }

        None
    }

    pub fn start_guest(self, capture: AppStateCapture, player: Option<String>) {
        match self.kind {
            RoomKind::Experimental { .. } => experimental::start_guest(self, player, capture),
            RoomKind::TerracottaLegacy { .. } | RoomKind::PCL2CE { .. } => legacy::start_guest(self, capture),
        };
    }
}
