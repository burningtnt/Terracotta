use crate::controller::states::{AppState, AppStateCapture};
use crate::controller::{ExceptionType, Room, RoomKind};
use crate::easytier;
use crate::mc::fakeserver::FakeServer;
use num_bigint::BigUint;
use socket2::{Domain, SockAddr, Socket, Type};
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::thread;
use std::time::{Duration, SystemTime};
use crate::ports::PortRequest;

pub fn parse(code: &str) -> Option<Room> {
    let chars: Vec<char> = code.to_ascii_uppercase().chars().collect();
    if chars.len() >= 29 {
        for start in 0..=(chars.len() - 29) {
            if let Some(room) = parse_segment(&chars[start..start + 29]) {
                return Some(room);
            }
        }
    }

    None
}

fn parse_segment(chars: &[char]) -> Option<Room> {
    static CHARS: &[u8] = "0123456789ABCDEFGHJKLMNPQRSTUVWXYZ".as_bytes();

    fn lookup_char(char: char) -> Option<u8> {
        let char = match char {
            'I' => '1',
            'O' => '0',
            _ => char,
        };

        for j in 0..34 {
            if CHARS[j] as char == char {
                return Some(j as u8);
            }
        }

        return None;
    }

    let mut array: [u8; 25] = [0; 25];
    for i in 0..5 {
        for j in 0..5 {
            if let Some(char) = lookup_char(chars[i * 6 + j]) {
                array[i * 5 + j] = char;
            } else {
                return None;
            }
        }

        if i != 4 && chars[i * 6 + 5] != '-' {
            return None;
        }
    }

    let mut checking: u8 = 0;
    for i in 0..24 {
        checking = (checking + array[i]) % 34;
    }
    if checking != array[24] {
        return None;
    }

    Some(Room {
        code: {
            let mut code = String::with_capacity(29);
            for i in 0..25 {
                code.push(CHARS[array[i] as usize] as char);
                if i == 4 || i == 9 || i == 14 || i == 19 {
                    code.push('-');
                }
            }
            code
        },
        network_name: {
            let mut name: [u8; 15] = [0; 15];
            for i in 0..15 {
                name[i] = CHARS[array[i] as usize];
            }
            name.make_ascii_lowercase();
            "terracotta-mc-".to_string() + str::from_utf8(&name).unwrap()
        },
        network_secret: {
            let mut secret: [u8; 10] = [0; 10];
            for i in 0..10 {
                secret[i] = CHARS[array[i + 15] as usize];
            }
            secret.make_ascii_lowercase();
            String::from_utf8(secret.to_vec()).unwrap()
        },
        kind: RoomKind::TerracottaLegacy {
            mc_port: {
                let mut value = BigUint::ZERO;
                for i in 0..25 {
                    // floor(log(34, 65536)) = 4
                    value += BigUint::from(34u8).pow(i as u32) * array[i];
                }

                (value % (65536u32)).try_into().unwrap()
            },
        },
    })
}

fn check_easytier() -> bool {
    let mut state = AppState::acquire();
    if let AppState::GuestStarting { easytier, .. }
    | AppState::GuestOk { easytier, .. } = state.as_mut_ref() && !easytier.is_alive()
    {
        logging!("Legacy Room", "EasyTier has crashed.");
        state.set(AppState::Exception {
            kind: ExceptionType::GuestEasytierCrash,
        });
        return true;
    }

    false
}

pub fn check_mc_conn(port: u16) -> bool {
    let start = SystemTime::now();

    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(64)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(64)))
        .unwrap();
    if let Ok(_) = socket.connect_timeout(
        &SockAddr::from(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port)),
        Duration::from_secs(64),
    ) && let Ok(_) = socket.send(&[0xFE]) {
        let mut buf: [MaybeUninit<u8>; _] = [MaybeUninit::uninit(); 1];

        if let Ok(size) = socket.recv(&mut buf)
            && size >= 1
            // SAFETY: The first byte has been initialized by recv, as size >= 1
            && unsafe { buf[0].assume_init() } == 0xFF
        {
            return true;
        }
    }

    thread::sleep(
        (start + Duration::from_secs(5))
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO),
    );
    false
}

pub fn start_guest(room: Room, capture: AppStateCapture) {
    static REPLAY_SERVERS: [&'static str; 10] = [
        "tcp://public.easytier.top:11010",
        "tcp://ah.nkbpal.cn:11010",
        "tcp://turn.hb.629957.xyz:11010",
        "tcp://turn.js.629957.xyz:11012",
        "tcp://sh.993555.xyz:11010",
        "tcp://turn.bj.629957.xyz:11010",
        "tcp://et.sh.suhoan.cn:11010",
        "tcp://et-hk.clickor.click:11010",
        "tcp://et.01130328.xyz:11010",
        "tcp://et.gbc.moe:11011",
    ];
    static DEFAULT_ARGUMENTS: [&'static str; 5] = [
        "--no-tun",
        "--compression=zstd",
        "--multi-thread",
        "--latency-first",
        "--enable-kcp-proxy",
    ];

    let mut args = vec![
        "--network-name".to_string(),
        room.network_name.clone(),
        "--network-secret".to_string(),
        room.network_secret.clone(),
    ];

    if matches!(room.kind, RoomKind::PCL2CE { .. }) {
        args.push("-p".to_string());
        args.push("tcp://43.139.42.188:11010".to_string())
    }

    for replay in REPLAY_SERVERS.iter() {
        args.push("-p".to_string());
        args.push(replay.to_string());
    }
    for arg in DEFAULT_ARGUMENTS.iter() {
        args.push(arg.to_string());
    }

    let local_port = PortRequest::Minecraft.request();

    let (host_ip, remote_port) = match room.kind {
        RoomKind::TerracottaLegacy { mc_port, .. } => ("10.144.144.1", mc_port),
        RoomKind::PCL2CE { mc_port, .. } => ("10.114.51.41", mc_port),
        _ => panic!("Should NOT be here"),
    };

    args.push("-d".to_string());
    args.push(format!(
        "--port-forward=tcp://[::0]:{}/{}:{}",
        local_port, host_ip, remote_port
    ));
    args.push(format!(
        "--port-forward=tcp://0.0.0.0:{}/{}:{}",
        local_port, host_ip, remote_port
    ));

    let capture = {
        let easytier = easytier::FACTORY.create(args);

        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::GuestStarting { room, easytier })
    };

    'init_conn: {
        for _ in 0..5 {
            if check_easytier() {
                return;
            }

            if check_mc_conn(local_port) {
                logging!("Legacy Room", "Connection to MC server has gone.");
                break 'init_conn;
            }
        }

        let Some(state) = capture.try_capture() else {
            return;
        };
        state.set(AppState::Exception {
            kind: ExceptionType::PingHostFail,
        });
        return;
    }

    let capture = {
        let server = FakeServer::create(local_port, crate::MOTD);
        let Some(state) = capture.try_capture() else {
            return;
        };

        state.replace(move |state| match state {
            AppState::GuestStarting { room, easytier } => AppState::GuestOk {
                room,
                easytier,
                server,
                profiles: vec![],
            },
            _ => unreachable!(),
        })
    };

    let mut count = 0;
    loop {
        if check_easytier() {
            return;
        }

        if check_mc_conn(local_port) {
            count = 0;

            if !capture.can_capture() {
                return;
            }
        } else {
            count += 1;

            if count >= 3 {
                logging!("Legacy Room", "Connection to MC server has gone.");
                let Some(state) = capture.try_capture() else {
                    return;
                };
                state.set(AppState::Exception {
                    kind: ExceptionType::PingHostRst,
                });
                return;
            }
        }
    }
}
