use crate::controller::states::AppState;
use crate::scaffolding::profile::{ProfileKind, ProfileSnapshot};
use crate::scaffolding::server::Handlers;
use crate::scaffolding::PacketResponse;
use serde::ser::SerializeSeq;
use serde::Serializer as _;
use serde_json::{json, Serializer, Value};
use std::io;
use std::time::SystemTime;

fn parse<F, R>(f: F) -> io::Result<R>
where
    F: FnOnce() -> Option<R>,
{
    f().ok_or(io::Error::from(io::ErrorKind::InvalidInput))
}

macro_rules! define_handle {
    ($namespace:ident : $path:ident [ $request:ident => $response:ident ] $($tokens:tt)* ) => {
        (
            ::core::stringify!($namespace),
            ::core::stringify!($path),
            #[allow(unused_variables, unused_mut)]
            |$request: &[u8], mut $response: Vec<u8>| -> ::std::io::Result<crate::scaffolding::PacketResponse> {
                $($tokens)*

                crate::scaffolding::PacketResponse::ok($response)
            }
        )
    };
}

pub static HANDLERS: Handlers = &[
    define_handle! { c:ping[request => response]
        response.extend_from_slice(request);
    },
    define_handle! { c:protocols[request => response]
        for (i, handler) in HANDLERS.iter().enumerate() {
            response.extend_from_slice(handler.0.as_bytes());
            response.push(b':');
            response.extend_from_slice(handler.1.as_bytes());

            if i != HANDLERS.len() - 1 {
                response.push(b'\0');
            }
        }
    },
    define_handle! { c:server_port[request => response]
        if let Some(port) = {
            let state = AppState::acquire();
            match state.as_ref() {
                AppState::HostOk { port, .. } => Some(*port),
                _ => None,
            }
        } {
            response.extend_from_slice(&port.to_be_bytes());
        } else {
            return PacketResponse::fail(32, response);
        }
    },
    define_handle! { c:player_ping[request => response]
        let value: Value = serde_json::from_str(&String::from_utf8_lossy(request))?;

        let name = parse(|| value.as_object()?.get("name")?.as_str())?;
        let machine_id = parse(|| value.as_object()?.get("machine_id")?.as_str())?;
        let vendor = parse(|| value.as_object()?.get("vendor")?.as_str())?;

        let mut container = AppState::acquire();
        let AppState::HostOk { profiles, .. } = container.as_mut_ref() else {
            return Err(io::Error::other("IllegalStateException: Expecting HostOk."));
        };
        match profiles.iter().position(|profile| profile.1.get_machine_id() == machine_id) {
            Some(i) if i >= 1 => {
                profiles[i].0 = SystemTime::now();

                if profiles[i].1.get_name() != name {
                    profiles[i].1.set_name(name.to_string());
                    container.increase_shared();
                }
            }
            Some(_) => return Err(io::Error::other("IllegalStateException: Cannot modify host, machine_id may conflict.")),
            None => {
                profiles.push((SystemTime::now(), ProfileSnapshot {
                    machine_id: machine_id.to_string(),
                    name: name.to_string(),
                    vendor: vendor.to_string(),
                    kind: ProfileKind::GUEST
                }.into_profile()));
                container.increase_shared();
            }
        }
    },
    define_handle! { c:player_profiles_list[request => response]
        let mut value = Serializer::new(&mut response);

        let container = AppState::acquire();
        let AppState::HostOk { profiles, .. } = container.as_ref() else {
            return Err(io::Error::other("IllegalStateException: Expecting HostOk."));
        };

        let mut sequence = value.serialize_seq(Some(profiles.len()))?;
        for (_, profile) in profiles {
            sequence.serialize_element(&json!({
                "name": profile.get_name(),
                "machine_id": profile.get_machine_id(),
                "vendor": profile.get_vendor(),
                "kind": match profile.get_kind() {
                    ProfileKind::HOST => "HOST",
                    ProfileKind::GUEST => "GUEST",
                    ProfileKind::LOCAL => unreachable!(),
                }
            }))?;
        }
        sequence.end()?;
    },
];