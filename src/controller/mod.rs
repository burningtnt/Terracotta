mod states;
mod api;
mod rooms;

pub use rooms::*;

pub use states::ExceptionType;
pub use api::*;
use crate::controller::rooms::scaffolding::protocols::HANDLERS;

lazy_static::lazy_static! {
    pub static ref SCAFFOLDING_PORT: u16 = crate::scaffolding::server::start(HANDLERS, 13448)
        .unwrap_or_else(|_| crate::scaffolding::server::start(HANDLERS, 0).unwrap());
}
