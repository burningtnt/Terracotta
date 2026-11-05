use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Cursor},
    path::Path,
};

const LOCK_FILE: &str = "cache.lock";

pub(super) fn prepare(dir: &Path, archive: &[u8], checksums: &str) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE))?;
    lock.try_lock()?;

    let files: Vec<_> = checksums
        .lines()
        .map(|line| {
            line.split_once(' ')
                .expect("Invalid embedded EasyTier checksum")
        })
        .collect();
    let mut invalid = Vec::new();
    for &(checksum, name) in &files {
        if !matches_checksum(&dir.join(name), checksum)? {
            invalid.push((checksum, name));
        }
    }
    if !invalid.is_empty() {
        // Extract separately so an interrupted extraction cannot leave partial executables in the cache.
        let staging = dir.join("extracting");
        sevenz_rust2::decompress(Cursor::new(archive), &staging).map_err(io::Error::other)?;
        for (checksum, name) in &invalid {
            if !matches_checksum(&staging.join(name), checksum)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Embedded EasyTier file failed checksum validation: {}",
                        name
                    ),
                ));
            }
        }
        for (_, name) in invalid {
            fs::rename(staging.join(name), dir.join(name))?;
        }
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == LOCK_FILE || files.iter().any(|(_, file)| name == *file) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    // Keep the lock for as long as the factory uses these files.
    Ok(lock)
}

fn matches_checksum(path: &Path, checksum: &str) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    }
    let mut hasher = Sha256::new();
    let mut reader = BufReader::new(File::open(path)?);
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        hasher.update(buffer);
        let length = buffer.len();
        reader.consume(length);
    }
    Ok(hex::encode(hasher.finalize()) == checksum)
}
