use std::{
    env, fs,
    io::{BufRead, BufReader, Cursor, Error, ErrorKind},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    thread,
};

static EASYTIER_ARCHIVE: (&'static str, &'static [u8]) = (
    include_str!(env!("TERRACOTTA_ET_ENTRY_CONF")), 
    include_bytes!(env!("TERRACOTTA_ET_ARCHIVE"))
);

lazy_static::lazy_static! {
    pub static ref FACTORY: EasytierFactory = create();
}

pub struct EasytierFactory {
    exe: PathBuf,
}

pub struct Easytier {
    process: process::Child,
}

fn create() -> EasytierFactory {
    let dir = Path::join(&env::temp_dir(), format!("terracotta-rs-{}", process::id()));

    let _ = fs::remove_dir_all(dir.clone());
    let _ = fs::create_dir_all(dir.clone());

    logging!(
        "Easytier",
        "Releasing easytier to {}",
        dir.to_string_lossy()
    );

    sevenz_rust2::decompress(Cursor::new(EASYTIER_ARCHIVE.1.to_vec()), dir.clone())
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
        .unwrap();

    let exe: PathBuf = Path::join(&dir, EASYTIER_ARCHIVE.0);
    #[cfg(target_family="unix")] {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(exe.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(exe.clone(), permissions).unwrap();
    }
    return EasytierFactory { exe: exe };
}

impl Drop for EasytierFactory {
    fn drop(&mut self) {
        let dir = self.exe.parent();
        if let Some(dir) = dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl EasytierFactory {
    pub fn create(&self, args: Vec<String>) -> Easytier {
        logging!("Easytier", "Starting easytier: {:?}", args);

        let mut process: process::Child = Command::new(self.exe.as_path())
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let stdout = process.stdout.take().unwrap();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    logging!("Easytier Core/STDOUT", "{}", line);
                }
            }
        });

        let stderr = process.stderr.take().unwrap();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    logging!("Easytier Core/STDERR", "{}", line);
                }
            }
        });

        return Easytier { process: process };
    }
}

impl Easytier {
    pub fn kill(mut self) {
        let _ = self.process.kill();
    }

    pub fn is_alive(&mut self) -> bool {
        if let Ok(value) = self.process.try_wait() {
            if let Some(_) = value {
                return false;
            } else {
                return true;
            }
        } else {
            return false;
        }
    }
}

impl Drop for Easytier {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
