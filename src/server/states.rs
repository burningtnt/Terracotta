use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::Value;

use crate::controller;
use crate::controller::Room;

#[get("/")]
fn get_state() -> Json<Value> {
    Json(controller::get_state())
}

#[get("/ide")]
fn set_state_ide() -> Status {
    controller::set_waiting();
    Status::Ok
}

#[get("/scanning?<room>&<player>")]
fn set_state_scanning(room: Option<String>, player: Option<String>) -> Status {
    controller::set_scanning(room, player);
    Status::Ok
}

#[get("/guesting?<room>&<player>")]
fn set_state_guesting(room: Option<String>, player: Option<String>) -> Status {
    if let Some(room) = room
        && let Some(room) = Room::from(&room)
        && controller::set_guesting(room, player)
    {
        return Status::Ok;
    }

    Status::BadRequest
}

pub fn configure(rocket: rocket::Rocket<rocket::Build>) -> rocket::Rocket<rocket::Build> {
    rocket.mount(
        "/state",
        routes![
            get_state,
            set_state_ide,
            set_state_scanning,
            set_state_guesting,
        ],
    )
}
