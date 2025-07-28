use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs,
    io::{Read, Write},
    path, thread,
    time::Duration,
};

use crate::FILE_ROOT;

pub enum State {
    Single { file: fs::File },
    Secondary { port: u16 },
    Unknown,
}

lazy_static::lazy_static! {
    static ref LOCK: path::PathBuf = FILE_ROOT.join("terracotta.lock");
}

impl State {
    pub fn get_state() -> State {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open((*LOCK).clone())
        {
            Ok(f) => {
                logging!("Lock", "Successfully hold the global mutex.");
                return State::Single { file: f };
            }
            Err(_) => {
                for i in (0..10).rev() {
                    match fs::OpenOptions::new()
                        .write(true)
                        .truncate(false)
                        .read(true)
                        .share_mode(3)
                        .open((*LOCK).clone())
                    {
                        Ok(mut f) => {
                            let mut buf: [u8; 2] = [0; 2];
                            if let Ok(size) = f.read(&mut buf)
                                && size == 2
                            {
                                let port = ((buf[0] as u16) << 8) + buf[1] as u16;
                                logging!(
                                    "Lock",
                                    "Successfully join the global mutex, port = {}",
                                    port
                                );
                                return State::Secondary { port: port };
                            } else {
                                logging!("Lock", "Global mutex is broken.");
                                return State::Unknown;
                            }
                        }
                        Err(err) => {
                            logging!(
                                "Lock",
                                "Cannot join the global mutex, cas = {}, err = {:?}",
                                i,
                                err
                            );
                        }
                    }

                    thread::sleep(Duration::from_millis(1000));
                }

                logging!(
                    "Lock",
                    "Having waited for 10s for the global mutx, which is still locked."
                );
                return State::Unknown;
            }
        };
    }

    pub fn set_port(self, port: u16) {
        {
            let State::Single { mut file } = self else {
                panic!("self must be State::Single.");
            };

            let _ = file.write_all(&[(port >> 8) as u8, (port & 0xFF) as u8]);
            let _ = file.sync_all();

            std::mem::drop(file);
        }

        if let Ok(file) = fs::OpenOptions::new()
            .write(true)
            .truncate(false)
            .read(true)
            .share_mode(3)
            .open((*LOCK).clone())
        {
            logging!("Lock", "Releasing global mutex lock. Turning into holders.");

            Box::leak(Box::new(file));
        } else {
            logging!("Lock", "Cannot release global mutex lock.");
        }
    }
}
