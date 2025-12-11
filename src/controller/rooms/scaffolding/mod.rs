mod room;
pub mod protocols;

use crate::MACHINE_ID_FILE;
use rand_core::{OsRng, TryRngCore};
pub use room::*;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

lazy_static::lazy_static! {
    pub static ref MACHINE_ID: &'static str = get_machine_id();

    static ref VENDOR: &'static str = format!("Terracotta {}, EasyTier {}", env!("TERRACOTTA_VERSION"), env!("TERRACOTTA_ET_VERSION")).leak();
}

fn get_machine_id() -> &'static str {
    if let Ok(mut file) = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(MACHINE_ID_FILE.clone()) {
        let mut bytes = [0u8; 17];
        match file.read(&mut bytes) {
            Ok(16) => {},
            Ok(length) => {
                logging!("MachineID", "Cannot restore machine id: expecting 16 bytes, but {} bytes are found.", length);
                OsRng.try_fill_bytes(&mut bytes[0..16]).unwrap();
                if let Ok(_) = file.seek(SeekFrom::Start(0)) {
                    let _ = file.write(&bytes[0..16]);
                }
            },
            Err(e) => {
                logging!("MachineID", "Cannot restore machine id: {:?}", e);
            },
        }

        return hex::encode(&bytes[0..16]).leak();
    }

    let mut bytes = [0u8; 17];
    OsRng.try_fill_bytes(&mut bytes).unwrap();
    return hex::encode(&bytes).leak();
}
