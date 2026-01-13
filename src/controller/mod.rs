mod states;
mod api;
mod rooms;

pub use rooms::*;
pub use states::*;
pub use api::*;

use crate::scaffolding::server::start as start;
use scaffolding::protocols::HANDLERS as HANDLERS;

lazy_static::lazy_static! {
    pub static ref SCAFFOLDING_PORT: u16 = start(HANDLERS, 13448).unwrap_or_else(|_| start(HANDLERS, 0).unwrap());
}
