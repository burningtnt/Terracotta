use libc::{LOCK_EX, LOCK_NB, LOCK_SH};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::io::AsRawFd,
    path,
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

fn flock(file: &fs::File, operation: i32) -> io::Result<()> {
    unsafe {
        if libc::flock(file.as_raw_fd(), operation) == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    return Ok(());
}

impl State {
    pub fn get_state() -> State {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open((*LOCK).clone())
            .unwrap();

        if flock(&file, LOCK_EX | LOCK_NB).is_ok() {
            file.set_len(0).unwrap();

            return State::Single { file: file };
        } else {
            let _ = flock(&file, LOCK_SH);

            let mut buf: [u8; 2] = [0; 2];
            if let Ok(size) = file.read(&mut buf)
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
    }

    pub fn set_port(self, port: u16) {
        let State::Single { mut file } = self else {
            panic!("self must be State::Single.");
        };

        let _ = file.write_all(&[(port >> 8) as u8, (port & 0xFF) as u8]);
        let _ = file.sync_all();
        let _ = flock(&file, LOCK_SH);

        Box::leak(Box::new(file));
    }
}
