use crate::controller::experimental::{MACHINE_ID, VENDOR};
use crate::controller::rooms::legacy;
use crate::controller::states::{AppState, AppStateCapture};
use crate::controller::{ExceptionType, Room, RoomKind, SCAFFOLDING_PORT};
use crate::easytier;
use crate::mc::fakeserver::FakeServer;
use crate::scaffolding::client::ClientSession;
use crate::scaffolding::profile::{Profile, ProfileKind, ProfileSnapshot};
use crate::scaffolding::PacketResponse;
use rand_chacha::ChaCha12Rng;
use rand_core::{OsRng, RngCore, SeedableRng, TryRngCore};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6, TcpListener};
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use std::{io, thread};
use crate::ports::PortRequest;

static CHARS: &[u8] = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ".as_bytes();

fn lookup_char(char: char) -> Option<u8> {
    let char = match char {
        'I' => '1',
        'O' => '0',
        _ => char,
    };

    for (j, c) in CHARS.iter().enumerate() {
        if *c as char == char {
            return Some(j as u8);
        }
    }

    None
}

pub fn create_room() -> Room {
    let value = {
        let mut bytes = [0u8; 16];
        OsRng.try_fill_bytes(&mut bytes).unwrap();
        u128::from_be_bytes(bytes)
    } % 34u128.pow(16);
    let value = value - value % 7;

    let (code, network_name, network_secret) = from_value(value);

    Room {
        code,
        network_name,
        network_secret,
        kind: RoomKind::Experimental { seed: value },
    }
}

pub fn parse(code: &str) -> Option<Room> {
    let code: Vec<char> = code.to_ascii_uppercase().chars().collect();
    if code.len() < "U/XXXX-XXXX-XXXX-XXXX".len() {
        return None;
    }

    let value: u128 = 'value: {
        'parse_segment: for code in code.windows("U/XXXX-XXXX-XXXX-XXXX".len()) {
            if code[0] != 'U' || code[1] != '/' {
                continue 'parse_segment;
            }

            let code = &code[2..];
            let mut value: u128 = 0;
            for i in (0.."XXXX-XXXX-XXXX-XXXX".len()).rev() {
                if i == 4 || i == 9 || i == 14 {
                    if code[i] != '-' {
                        continue 'parse_segment;
                    }
                } else {
                    match lookup_char(code[i]) {
                        Some(v) => value = value * 34 + v as u128,
                        None => continue 'parse_segment,
                    }
                }
            }
            if value.is_multiple_of(7) {
                break 'value value;
            }
        }
        return None;
    };

    let (code, network_name, network_secret) = from_value(value);

    Some(Room {
        code,
        network_name,
        network_secret,
        kind: RoomKind::Experimental { seed: value },
    })
}

fn from_value(value: u128) -> (String, String, String) {
    let mut code = String::with_capacity("U/XXXX-XXXX-XXXX-XXXX".len());
    code.push_str("U/");
    let mut network_name = String::with_capacity("scaffolding-mc-XXXX-XXXX".len());
    network_name.push_str("scaffolding-mc-");
    let mut network_secret = String::with_capacity("XXXX-XXXX".len());

    let mut value = value;
    for i in 0..16 {
        let v = CHARS[(value % 34) as usize] as char;
        value /= 34;

        if i == 4 || i == 8 || i == 12 {
            code.push('-');
        }
        code.push(v);

        if i < 8 {
            if i == 4 {
                network_name.push('-');
            }
            network_name.push(v);
        } else {
            if i == 12 {
                network_secret.push('-');
            }
            network_secret.push(v);
        }
    }

    assert_eq!(value, 0);
    assert_eq!(code.len(), "U/XXXX-XXXX-XXXX-XXXX".len());
    assert_eq!(network_name.len(), "scaffolding-mc-XXXX-XXXX".len());
    assert_eq!(network_secret.len(), "XXXX-XXXX".len());

    (code, network_name, network_secret)
}

pub fn start_host(room: Room, port: u16, player: Option<String>, capture: AppStateCapture) {
    let scaffolding = *SCAFFOLDING_PORT;

    let mut args = compute_arguments(&room);
    args.push("--hostname".to_string());
    args.push(format!("scaffolding-mc-server-{}", scaffolding));
    args.push("--ipv4".to_string());
    args.push("10.144.144.1".to_string());
    args.push(format!("--tcp-whitelist={}", scaffolding));
    args.push(format!("--tcp-whitelist={}", port));
    args.push(format!("--tcp-whitelist={}", port));

    let easytier = easytier::FACTORY.create(args);
    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::HostOk {
            room,
            port,
            easytier,
            profiles: vec![(
                SystemTime::now(),
                ProfileSnapshot {
                    machine_id: MACHINE_ID.to_string(),
                    name: player.unwrap_or("Terracotta Anonymous Host".to_string()),
                    vendor: VENDOR.to_string(),
                    kind: ProfileKind::HOST
                }.into_profile()
            )],
        })
    };

    thread::spawn(move || {
        let mut counter = 0;
        loop {
            if legacy::check_mc_conn(port) {
                counter = 0;
            } else {
                counter += 1;
                if counter >= 3 {
                    let Some(state) = capture.try_capture() else {
                        return;
                    };
                    state.set(AppState::Exception { kind: ExceptionType::PingServerRst });
                    return;
                }
            }

            let Some(mut state) = capture.try_capture() else {
                return;
            };
            let AppState::HostOk { easytier, profiles, .. } = state.as_mut_ref() else {
                unreachable!();
            };

            if !easytier.is_alive() {
                state.set(AppState::Exception { kind: ExceptionType::HostEasytierCrash });
                return;
            }

            let mut changed = false;
            let now = SystemTime::now();
            for i in (1..profiles.len()).rev() {
                let (time, profile) = &profiles[i];
                if i != 0 && now.duration_since(*time).is_ok_and(|d| d >= Duration::from_secs(10)) {
                    logging!("RoomExperiment", "Removing guest {}: timeout.", profile.get_name());
                    profiles.remove(i);
                    changed = true;
                }
            }
            if changed {
                state.increase_shared();
            }
        }
    });
}

pub fn start_guest(room: Room, player: Option<String>, capture: AppStateCapture) {
    let mut args = compute_arguments(&room);
    args.push("-d".to_string());
    args.push("--tcp-whitelist=0".to_string());
    args.push("--udp-whitelist=0".to_string());
    let easytier = easytier::FACTORY.create(args);
    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };

        state.set(AppState::GuestStarting { room, easytier })
    };

    let scaffolding = 'local_port: {
        for _ in 0..5 {
            thread::sleep(Duration::from_secs(3));

            let Some(state) = capture.try_capture() else {
                return;
            };
            let mut state = state.into_slow();
            let AppState::GuestStarting { easytier, .. } = state.as_mut_ref() else {
                unreachable!();
            };
            if !easytier.is_alive() {
                state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                return;
            }

            let Some(players) = easytier.get_players() else {
                continue;
            };
            for (hostname, ip) in players {
                if hostname.starts_with("scaffolding-mc-server-") && let Ok(port) = u16::from_str(&hostname["scaffolding-mc-server-".len()..]) {
                    logging!("RoomExperiment", "Scaffolding Server is at {}:{}", ip, port);

                    let local_port = PortRequest::Scaffolding.request();

                    if !easytier.add_port_forward(&[(
                        SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port).into(),
                        SocketAddrV4::new(ip, port).into(),
                    )]) {
                        logging!("RoomExperiment", "Cannot create a port-forward {} -> {} for Scaffolding Connection.", local_port, port);
                        state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                        return;
                    };

                    break 'local_port local_port;
                }
            }
        }

        logging!("RoomExperiment", "Cannot find scaffolding server.");
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::Exception { kind: ExceptionType::PingHostFail });
        return;
    };

    fn fail(capture: AppStateCapture) {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::Exception { kind: ExceptionType::PingHostFail });
    }

    let mut session = 'session: {
        for timeout in [4, 4, 8, 4, 8, 16] {
            thread::sleep(Duration::from_secs(timeout));

            const FINGERPRINT: [u8; 16] = [0x41, 0x57, 0x48, 0x44, 0x86, 0x37, 0x40, 0x59, 0x57, 0x44, 0x92, 0x43, 0x96, 0x99, 0x85, 0x01];
            if let Ok(mut session) = ClientSession::open(IpAddr::V4(Ipv4Addr::LOCALHOST), scaffolding)
                && let Some(response) = session.send_sync(("c", "ping"), |body| {
                body.extend_from_slice(&FINGERPRINT);
            })
            {
                let PacketResponse::Ok { data } = response else {
                    unreachable!();
                };

                if data.len() == 16 && data == FINGERPRINT {
                    logging!("RoomExperiment", "Scaffolding Server has been verified.");
                    break 'session session;
                }
            }

            let Some(mut state) = capture.try_capture() else {
                return;
            };
            let AppState::GuestStarting { easytier, .. } = state.as_mut_ref() else {
                unreachable!();
            };
            if !easytier.is_alive() {
                state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                return;
            }
        }

        logging!("RoomExperiment", "Cannot connect to scaffolding server.");
        fail(capture);
        return;
    };

    let Some(response) = session.send_sync(("c", "server_port"), |_| {}) else {
        fail(capture);
        return;
    };

    let port = if let PacketResponse::Ok { data } = response
        && data.len() == 2
    {
        let mut p = [0u8; 2];
        p.copy_from_slice(data.as_slice());
        u16::from_be_bytes(p)
    } else {
        fail(capture);
        return;
    };
    logging!("RoomExperiment", "MC server is at {}", port);

    let local_port = {
        let Some(state) = capture.try_capture() else {
            return;
        };
        let mut state = state.into_slow();
        let AppState::GuestStarting { easytier, .. } = state.as_mut_ref() else {
            unreachable!();
        };

        let local_port = PortRequest::Minecraft.request();

        if !easytier.add_port_forward(&[(
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port).into(),
            SocketAddrV4::new(Ipv4Addr::new(10, 144, 144, 1), port).into()
        ), (
            SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, local_port, 0, 0).into(),
            SocketAddrV4::new(Ipv4Addr::new(10, 144, 144, 1), port).into()
        )]) {
            logging!("RoomExperiment", "Cannot create a port-forward {} -> {} for MC Connection.", local_port, port);
            state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
            return;
        }

        local_port
    };

    for _ in 0..8 {
        if legacy::check_mc_conn(local_port) {
            break;
        }
    }
    logging!("RoomExperiment", "MC connection is OK.");

    let local = ProfileSnapshot {
        machine_id: MACHINE_ID.to_string(),
        name: player.unwrap_or("Terracotta Anonymous Guest".to_string()),
        vendor: VENDOR.to_string(),
        kind: ProfileKind::LOCAL,
    }.into_profile();

    let capture = {
        let Some(state) = capture.try_capture() else {
            return;
        };
        state.replace(|state| {
            let AppState::GuestStarting { room, easytier } = state else {
                unreachable!();
            };

            AppState::GuestOk {
                room,
                easytier,
                server: FakeServer::create(local_port, crate::MOTD),
                profiles: vec![local.clone()],
            }
        })
    };

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(5));

            {
                let Some(_) = session.send_sync(("c", "player_ping"), |body| {
                    serde_json::to_writer(body, &json!({
                        "machine_id": local.get_machine_id(),
                        "name": local.get_name(),
                        "vendor": local.get_vendor()
                    })).unwrap();
                }) else {
                    fail(capture);
                    return;
                };
            }

            {
                let Some(mut server_profiles) = session.send_sync(("c", "player_profiles_list"), |_| {}).map(|response| {
                    let PacketResponse::Ok { data } = response else {
                        unreachable!();
                    };
                    data
                }).and_then(|data| {
                    let mut host = false;
                    let mut local = false;

                    let mut server_players: Vec<Profile> = vec![];
                    for item in serde_json::from_slice::<Value>(data.as_slice()).ok()?.as_array()? {
                        let name = item.as_object()?.get("name")?.as_str()?;
                        let machine_id = item.as_object()?.get("machine_id")?.as_str()?;
                        let vendor = item.as_object()?.get("vendor")?.as_str()?;

                        let kind = if machine_id == *MACHINE_ID {
                            if local {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Multiple local player, machine_id may have conflicted.");
                                return None;
                            }
                            local = true;

                            ProfileKind::LOCAL
                        } else {
                            match item.as_object()?.get("kind")?.as_str()? {
                                "HOST" if !host => {
                                    host = true;
                                    ProfileKind::HOST
                                }
                                "GUEST" => ProfileKind::GUEST,
                                _ => return None,
                            }
                        };

                        server_players.push(ProfileSnapshot {
                            machine_id: machine_id.to_string(),
                            name: name.to_string(),
                            vendor: vendor.to_string(),
                            kind,
                        }.into_profile())
                    }
                    if !host {
                        logging!("RoomExperiment", "API c:player_profiles_list invocation failed: No host detected.");
                        return None;
                    }

                    server_players.sort_by_cached_key(|profile| profile.get_machine_id().to_string());
                    for profile in server_players.windows(2) {
                        if profile[0].get_machine_id() == profile[1].get_machine_id() {
                            logging!("RoomExperiment", "API c:player_profiles_list invocation failed: machine_id conflict.");
                            return None;
                        }
                    }
                    Some(server_players)
                }) else {
                    fail(capture);
                    return;
                };

                let Some(mut state) = capture.try_capture() else {
                    return;
                };
                let AppState::GuestOk { easytier, profiles, .. } = state.as_mut_ref() else {
                    unreachable!();
                };
                if !easytier.is_alive() {
                    state.set(AppState::Exception { kind: ExceptionType::GuestEasytierCrash });
                    return;
                }

                if server_profiles.binary_search_by_key(&*MACHINE_ID, |profile| profile.get_machine_id()).is_err() {
                    server_profiles.push(local.clone());
                }

                let mut used = vec![false; server_profiles.len()];
                let mut changed = false;
                for i in (0..profiles.len()).rev() {
                    let profile = &mut profiles[i];
                    match profile.get_kind() {
                        ProfileKind::HOST => match server_profiles.binary_search_by_key(&profile.get_machine_id(), |p| p.get_machine_id()) {
                            Ok(index) if !used[index] && *server_profiles[index].get_kind() == ProfileKind::HOST => {
                                used[index] = true;
                                if profile.get_name() != server_profiles[index].get_name() {
                                    profile.set_name(server_profiles[index].get_name().to_string());
                                    changed = true;
                                }
                            }
                            _ => {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Host Profile is consumed or invalid, machine_id may have conflict.");
                                state.set(AppState::Exception { kind: ExceptionType::ScaffoldingInvalidResponse });
                                return;
                            }
                        },
                        ProfileKind::LOCAL => {}
                        ProfileKind::GUEST => match server_profiles.binary_search_by_key(&profile.get_machine_id(), |p| p.get_machine_id()) {
                            Ok(index) if used[index] && *server_profiles[index].get_kind() == ProfileKind::GUEST => {
                                profiles.remove(i);
                                changed = true;
                            }
                            Ok(index) if *server_profiles[index].get_kind() == ProfileKind::GUEST => {
                                used[index] = true;
                                if profile.get_name() != server_profiles[index].get_name() {
                                    profile.set_name(server_profiles[index].get_name().to_string());
                                    changed = true;
                                }
                            }
                            Ok(_) => {
                                logging!("RoomExperiment", "API c:player_profiles_list invocation failed: Guest Profile type is changed, machine_id may have conflict.");
                                state.set(AppState::Exception { kind: ExceptionType::ScaffoldingInvalidResponse });
                                return;
                            }
                            Err(_) => {
                                profiles.remove(i);
                                changed = true;
                            }
                        },
                    }
                }
                for i in (0..server_profiles.len()).rev() {
                    let profile = server_profiles.pop().unwrap();
                    if !used[i] && *profile.get_kind() != ProfileKind::LOCAL {
                        profiles.push(profile);
                        changed = true;
                    }
                }
                if changed {
                    state.increase_shared();
                }
            }
        }
    });
}

static FALLBACK_SERVERS: [&str; 2] = [
    "tcp://public.easytier.top:11010",
    "tcp://public2.easytier.cn:54321",
];

fn compute_arguments(room: &Room) -> Vec<String> {
    static DEFAULT_ARGUMENTS: [&str; 7] = [
        "--no-tun",
        "--compression=zstd",
        "--multi-thread",
        "--latency-first",
        "--enable-kcp-proxy",
        "-l",
        "udp://0.0.0.0:0",
    ];

    let mut args: Vec<String> = Vec::with_capacity(32);
    args.extend_from_slice(&[
        "--network-name".to_string(),
        room.network_name.clone(),
        "--network-secret".to_string(),
        room.network_secret.clone(),
    ]);

    match fetch_public_nodes(room) {
        Ok(nodes) => {
            for replay in nodes {
                args.push("-p".to_string());
                args.push(replay);
            }
        }
        Err(e) => {
            logging!("RoomExperiment", "Cannot fetch EasyTier public nodes: {:?}.", e);
            for replay in FALLBACK_SERVERS {
                args.push("-p".to_string());
                args.push(replay.to_string());
            }
        }
    }

    for arg in DEFAULT_ARGUMENTS {
        args.push(arg.to_string());
    }
    args
}

fn fetch_public_nodes(room: &Room) -> io::Result<Vec<String>> {
    static LIMIT: usize = 5;

    let mut servers: Vec<String> = serde_json::from_reader::<_, Value>(
        reqwest::blocking::Client::builder()
            .timeout(Some(Duration::from_secs(10)))
            .build()
            .map_err(io::Error::other)?
            .get("https://uptime.easytier.cn/api/nodes?page=1&per_page=50&is_active=true")
            .send()
            .map_err(io::Error::other)?
    ).map_err(io::Error::other)?
        .as_object()
        .and_then(|object| {
            if !object.get("success")?.as_bool()? {
                return None;
            }

            Some(object.get("data")?.as_object()?
                .get("items")?.as_array()?
                .iter().filter_map(|node| node.as_object())
                .flat_map(|node| {
                    let address = node.get("address")?.as_str()?;
                    if node.get("allow_relay")?.as_bool()? && node.get("is_active")?.as_bool()? && !FALLBACK_SERVERS.contains(&address) {
                        Some(address.to_string())
                    } else {
                        None
                    }
                })
                .collect())
        })
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;

    if servers.len() > LIMIT {
        let mut rng = ChaCha12Rng::from_seed(match room.kind {
            RoomKind::Experimental { seed } => {
                let mut value = [0u8; 32];
                value[0..16].copy_from_slice(&seed.to_be_bytes());
                value
            }
            _ => unreachable!(),
        });

        for i in (1..servers.len()).rev() {
            servers.swap(i, rng.next_u32() as usize % (i + 1));
        }
        servers.truncate(5);
    }
    for fallback in FALLBACK_SERVERS {
        servers.push(fallback.to_string());
    }
    Ok(servers)
}