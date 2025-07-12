use core::panic;

use num_bigint::BigUint;
use rand_core::{OsRng, TryRngCore};

use crate::{
    easytier::{Easytier, EasytierFactory, create_factory},
    fakeserver::{self, FakeServer},
};

#[derive(Debug)]
pub struct Room {
    pub name: [u8; 15],
    pub secret: [u8; 10],
    pub code: String,
    pub port: u16,
    pub host: bool,
}

pub const MOTD: &'static str = "§6§lTerracotta | 陶瓦 联机大厅（请关闭代理软件 否则无法进服）";
pub const LOCAL_PORT: u16 = 35781;

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

fn rem64(value: &BigUint) -> usize {
    return (value % (34 as u32)).try_into().unwrap();
}

impl Room {
    pub fn create(port: u16) -> Room {
        let mut buffer: [u8; 15] = [0; 15];
        OsRng.try_fill_bytes(&mut buffer).unwrap();

        let mut value = BigUint::ZERO;
        for i in 0..buffer.len() {
            value = (value << 8) + buffer[i];
        }

        value = value / (65536 as u32) * (65536 as u32) + port;

        let mut name: [u8; 15] = [0; 15];
        let mut secret: [u8; 10] = [0; 10];
        let mut checking: usize = 0;
        for i in 0..15 {
            name[i] = CHARS[rem64(&value)];
            checking = (rem64(&value) + checking) % 34;
            value /= 34 as u32;
        }
        for i in 0..9 {
            secret[i] = CHARS[rem64(&value)];
            checking = (rem64(&value) + checking) % 34;
            value /= 34 as u32;
        }
        secret[9] = CHARS[checking];

        if value != BigUint::ZERO {
            panic!("Cannot generate code: There's {} remained.", value);
        }

        let mut code: [u8; 29] = [0; 29];
        code[0..5].copy_from_slice(&name[0..5]);
        code[5] = b'-';
        code[6..11].copy_from_slice(&name[5..10]);
        code[11] = b'-';
        code[12..17].copy_from_slice(&name[10..15]);
        code[17] = b'-';
        code[18..23].copy_from_slice(&secret[0..5]);
        code[23] = b'-';
        code[24..29].copy_from_slice(&secret[5..10]);

        let room = Room {
            name: name,
            secret: secret,
            code: String::from_utf8(code.to_vec()).unwrap(),
            port: port,
            host: true,
        };

        return room;
    }

    pub fn from(code: &String) -> Result<Room, String> {
        let chars: Vec<char> = code.to_ascii_uppercase().chars().collect::<Vec<_>>();
        if chars.len() < 29 {
            return Err("Not enough data.".to_string());
        }

        let mut array: [u8; 25] = [0; 25];
        'moving: for start in 0..=(chars.len() - 29) {
            for i in 0..5 {
                for j in 0..5 {
                    if let Some(char) = lookup_char(chars[start + i * 6 + j]) {
                        array[i * 5 + j] = char;
                    } else {
                        continue 'moving;
                    }
                }

                if i != 4 && chars[start + i * 6 + 5] != '-' {
                    continue 'moving;
                }
            }

            let mut checking: u8 = 0;
            for i in 0..24 {
                checking = (checking + array[i]) % 34;
            }
            if checking != array[24] {
                continue 'moving;
            }

            return Ok(Room {
                name: {
                    let mut name: [u8; 15] = [0; 15];
                    for i in 0..15 {
                        name[i] = CHARS[array[i] as usize];
                    }
                    name
                },
                secret: {
                    let mut secret: [u8; 10] = [0; 10];
                    for i in 0..10 {
                        secret[i] = CHARS[array[i + 15] as usize];
                    }
                    secret
                },
                code: {
                    let mut code = String::from("");
                    for i in 0..25 {
                        code.push(CHARS[array[i] as usize] as char);
                        if i == 4 || i == 9 || i == 14 || i == 19 {
                            code.push('-');
                        }
                    }
                    code
                },
                port: {
                    let mut value = BigUint::ZERO;
                    for i in 0..25 {
                        // floor(log(34, 65536)) = 4
                        value += BigUint::from(34 as u8).pow(i as u32) * array[i];
                    }

                    (value % (65536 as u32)).try_into().unwrap()
                },
                host: false,
            });
        }

        return Err("No Room code found.".to_string());
    }

    pub fn start(&self) -> (Easytier, Option<FakeServer>) {
        lazy_static::lazy_static! {
            static ref factory: EasytierFactory = create_factory().unwrap();
        }

        let mut args = vec![
            "--network-name".to_string(),
            format!(
                "terracotta-mc-{}",
                String::from_utf8_lossy(&self.name).to_ascii_lowercase()
            ),
            "--network-secret".to_string(),
            String::from_utf8_lossy(&self.secret).to_ascii_lowercase(),
        ];

        args.extend(
            vec![
                "-p",
                "tcp://public.easytier.cn:11010",
                "--no-tun",
                "--compression=zstd",
                "--multi-thread",
                "--latency-first",
            ]
            .iter()
            .map(|n| n.to_string()),
        );

        args.append(
            &mut (if self.host {
                vec!["--ipv4".to_string(), "10.144.144.1".to_string()]
            } else {
                vec![
                    "-d".to_string(),
                    format!(
                        "--port-forward=tcp://0.0.0.0:{}/10.144.144.1:{}",
                        LOCAL_PORT, self.port
                    ),
                    format!(
                        "--port-forward=tcp://[::0]:{}/10.144.144.1:{}",
                        LOCAL_PORT, self.port
                    ),
                ]
            }),
        );

        return (
            factory.create(args),
            if self.host {
                None
            } else {
                let s = fakeserver::create(MOTD.to_string());
                s.set_port(LOCAL_PORT);
                Some(s)
            },
        );
    }
}
