use std::{
    env, fs,
    io::{BufRead, BufReader, Cursor, Error, ErrorKind},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use crate::EASYTIER_DIR;

static EASYTIER_ARCHIVE: (&'static str, &'static [u8]) = (
    include_str!(env!("TERRACOTTA_ET_ENTRY_CONF")),
    include_bytes!(env!("TERRACOTTA_ET_ARCHIVE")),
);

lazy_static::lazy_static! {
    pub static ref FACTORY: EasytierFactory = create();
}

pub struct EasytierFactory {
    exe: PathBuf,
}

pub struct Easytier {
    process: Arc<Mutex<Child>>,
}

fn create() -> EasytierFactory {
    let _ = fs::create_dir_all(&*EASYTIER_DIR);

    logging!(
        "Easytier",
        "Releasing easytier to {}",
        &*EASYTIER_DIR.to_string_lossy()
    );

    sevenz_rust2::decompress(Cursor::new(EASYTIER_ARCHIVE.1.to_vec()), &*EASYTIER_DIR)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
        .unwrap();

    let exe: PathBuf = Path::join(&*EASYTIER_DIR, EASYTIER_ARCHIVE.0);
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(exe.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(exe.clone(), permissions).unwrap();
    }
    return EasytierFactory { exe: exe };
}

impl Drop for EasytierFactory {
    fn drop(&mut self) {
        self.drop_in_place();
    }
}

impl EasytierFactory {
    pub fn create(&self, args: Vec<String>) -> Easytier {
        logging!("Easytier", "Starting easytier: {:?}", args);

        fs::metadata(&self.exe).unwrap();

        let mut process = Command::new(self.exe.as_path());
        process.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_family = "windows")]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x08000000);
        }

        let mut process = process.spawn().unwrap();

        let (sender, receiver) = mpsc::channel::<String>();
        let pump = [
            (
                "stdout",
                Self::pump_std(&sender, process.stdout.take().unwrap()),
            ),
            (
                "stderr",
                Self::pump_std(&sender, process.stderr.take().unwrap()),
            ),
        ];

        let process: Arc<Mutex<Child>> = Arc::new(Mutex::new(process));
        let process2 = process.clone();

        thread::spawn(move || {
            const LINES: usize = 500;

            let mut buffer: [Option<String>; LINES] = [const { None }; LINES];
            let mut index = 0;

            let status = loop {
                {
                    let mut process = process2.lock().unwrap();
                    if let Ok(value) = process.try_wait() {
                        if let Some(_) = value {
                            break value;
                        }
                    } else {
                        break None;
                    }
                }

                if let Ok(value) = receiver.try_recv() {
                    buffer[index] = Some(value);
                    index = (index + 1) % LINES;
                }

                thread::sleep(Duration::from_millis(100));
            };

            thread::sleep(Duration::from_secs(3));
            for (name, join) in pump.into_iter() {
                if !join.is_finished() {
                    logging!("UI", "Logging adapter {} has hang for 3s.", name);
                }
            }

            let mut output = String::from("Easytier has exit. with status {");
            output += &match status {
                Some(status) => format!(
                    "code={}, success={}",
                    status
                        .code()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    status.success()
                ),
                None => "unknown".to_string(),
            };
            output.push_str("}. Here's the logs:\n---------------");
            for i in 0..LINES {
                if let Some(value) = &buffer[(index + 1 + i) % LINES] {
                    output.push('\n');
                    output.push_str(&value);
                }
            }
            output.push_str("\n---------------");

            logging!("Easytier Core", "{}", output);
        });

        return Easytier { process: process };
    }

    fn pump_std<R: std::io::Read + std::marker::Send + 'static>(
        sender: &mpsc::Sender<String>,
        source: R,
    ) -> thread::JoinHandle<()> {
        let sender = sender.clone();
        return thread::spawn(move || {
            let reader = BufReader::new(source);
            for line in reader.lines() {
                if let Ok(line) = line {
                    sender.send(line).unwrap();
                }
            }
        });
    }

    pub fn drop_in_place(&self) {
        let dir = self.exe.parent();
        if let Some(dir) = dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl Easytier {
    pub fn kill(self) {
        let _ = self.process.lock().unwrap().kill();
    }

    pub fn is_alive(&mut self) -> bool {
        if let Ok(value) = self.process.lock().unwrap().try_wait() {
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
        let _ = self.process.lock().unwrap().kill();
    }
}
