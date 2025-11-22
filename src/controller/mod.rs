mod states;
mod api;
mod rooms;

use crate::scaffolding;

pub use rooms::*;
pub use states::*;
pub use api::*;

lazy_static::lazy_static! {
    pub static ref SCAFFOLDING_PORT: u16 = scaffolding::server::start(experimental::HANDLERS, 13448)
        .unwrap_or_else(|_| scaffolding::server::start(experimental::HANDLERS, 0).unwrap());
}
