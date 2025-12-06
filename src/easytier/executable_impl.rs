use crate::easytier::argument::{Argument, PortForward};
use crate::ports::PortRequest;
use crate::EASYTIER_DIR;
use parking_lot::Mutex;
use std::ffi::OsString;
use std::fmt::Write;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::{
    env, fs,
    io::{BufRead, BufReader, Cursor, Error},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use crate::easytier::{EasyTierMember, NatType};

static EASYTIER_ARCHIVE: (&str, &str, &[u8]) = (
    include_str!(env!("TERRACOTTA_ET_ENTRY_CONF")),
    include_str!(env!("TERRACOTTA_ET_CLI_CONF")),
    include_bytes!(env!("TERRACOTTA_ET_ARCHIVE")),
);

lazy_static::lazy_static! {
    static ref FACTORY: EasytierFactory = create_factory();
}

struct EasytierFactory {
    exe: PathBuf,
    cli: PathBuf,
}

pub fn initialize() {
    lazy_static::initialize(&FACTORY);
}

pub struct EasyTier {
    process: Arc<Mutex<Child>>,
    rpc: u16,
}

fn create_factory() -> EasytierFactory {
    let _ = fs::create_dir_all(&*EASYTIER_DIR);

    logging!(
        "Easytier",
        "Releasing easytier to {}",
        &*EASYTIER_DIR.to_string_lossy()
    );

    sevenz_rust2::decompress(Cursor::new(EASYTIER_ARCHIVE.2.to_vec()), &*EASYTIER_DIR)
        .map_err(|e| Error::other(e.to_string()))
        .unwrap();

    let exe: PathBuf = Path::join(&EASYTIER_DIR, EASYTIER_ARCHIVE.0);
    let cli: PathBuf = Path::join(&EASYTIER_DIR, EASYTIER_ARCHIVE.1);
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(exe.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(exe.clone(), permissions).unwrap();

        let mut permissions = fs::metadata(cli.clone()).unwrap().permissions();
        permissions.set_mode(permissions.mode() | 0o100);
        fs::set_permissions(cli.clone(), permissions).unwrap();
    }
    EasytierFactory { exe, cli }
}

pub fn create(args: Vec<Argument>) -> EasyTier {
    let args = {
        let mut built: Vec<OsString> = Vec::with_capacity((args.len() as f32 * 1.5).floor() as usize);

        macro_rules! push {
                ($($item:expr),* $(,)?) => {
                    built.extend_from_slice(&[$($item.into()),*])
                };
            }
        for arg in args {
            match arg {
                Argument::NoTun => push!["--no-tun"],
                Argument::Compression(method) => push![format!("--compression={}", method)],
                Argument::MultiThread => push!["--multi-thread"],
                Argument::LatencyFirst => push!["--latency-first"],
                Argument::EnableKcpProxy => push!["--enable-kcp-proxy"],
                Argument::PublicServer(server) => push!["-p", server.as_ref()],
                Argument::NetworkName(name) => push!["--network-name", name.as_ref()],
                Argument::NetworkSecret(secret) => push!["--network-secret", secret.as_ref()],
                Argument::Listener { address, proto } => push!["-l", format!("{}://{}", proto.name(), address)],
                Argument::PortForward(PortForward { local, remote, proto }) => push![
                        format!("--port-forward={}://{}/{}", proto.name(), local, remote)
                    ],
                Argument::DHCP => push!["-d"],
                Argument::HostName(name) => push!["--hostname", name.as_ref()],
                Argument::IPv4(address) => push!["--ipv4", address.to_string()],
                Argument::TcpWhitelist(port) => push![format!("--tcp-whitelist={}", port)],
                Argument::UdpWhitelist(port) => push![format!("--udp-whitelist={}", port)],
                Argument::P2POnly => push!["--p2p-only"],
            }
        }
        built
    };

    fs::metadata(&FACTORY.exe).unwrap();

    let rpc = PortRequest::EasyTierRPC.request();

    logging!("Easytier", "Starting easytier: {:?}, rpc={}", args, rpc);

    let mut process = Command::new(FACTORY.exe.as_path());
    process
        .args(args)
        .args(["-r", &rpc.to_string()])
        .current_dir(env::temp_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_family = "windows")]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x08000000);
    }

    let mut process = process.spawn().unwrap();

    let (sender, receiver) = mpsc::channel::<String>();
    forward_std(&mut process, move |line| {
        let _ = sender.send(line);
    });

    let process: Arc<Mutex<Child>> = Arc::new(Mutex::new(process));
    let process2 = process.clone();

    thread::spawn(move || {
        const LINES: usize = 500;

        let mut buffer: [Option<String>; LINES] = [const { None }; LINES];
        let mut index = 0;

        let status = 'status: loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(value) => {
                    buffer[index] = Some(value);
                    index = (index + 1) % LINES;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    match process2.lock().try_wait() {
                        Ok(Some(status)) => break 'status Some(status),
                        Ok(None) => {
                            logging!("EasyTier", "Cannot fetch EasyTier process status: EasyTier hasn't exited.");
                        }
                        Err(e) => {
                            logging!("EasyTier", "Cannot fetch EasyTier process status: {:?}", e);
                        }
                    }
                    break 'status None;
                }
            }
        };

        let mut output = String::from("Easytier has exited. with status ");
        match status {
            Some(status) => match status.code() {
                Some(code) => write!(output, "code={}, success={}", code, status.success()),
                None => write!(output, "code=[unknown], success={}", status.success()),
            }.unwrap(),
            None => output.push_str("[unknown]"),
        }
        output.push_str(". Here's the logs:\n############################################################");
        for i in 0..LINES {
            if let Some(value) = &buffer[(index + 1 + i) % LINES] {
                output.push_str("\n    ");
                output.push_str(value);
            }
        }
        output.push_str("\n############################################################");

        logging!("Easytier", "{}", output);
    });

    EasyTier { process, rpc }
}

pub fn cleanup() {
    if let Some(dir) = FACTORY.exe.parent() {
        let _ = fs::remove_dir_all(dir);
    }
}

fn forward_std<F>(process: &mut Child, handle: F)
where
    F: Fn(String) + Send + Sized + Clone + 'static,
{
    let handle2 = handle.clone();

    let stdout = process.stdout.take().unwrap();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            handle(line);
        }
    });

    let stderr = process.stderr.take().unwrap();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            handle2(line);
        }
    });
}

impl EasyTier {
    pub fn is_alive(&self) -> bool {
        matches!(self.process.lock().try_wait(), Ok(None))
    }

    pub fn get_players(&self) -> Option<Vec<EasyTierMember>> {
        let object: serde_json::Value = serde_json::from_str(std::str::from_utf8(
            &self.start_cli()
                .args(["-p", &format!("127.0.0.1:{}", self.rpc), "-o", "json", "peer"])
                .output().ok()?.stdout
        ).ok()?).ok()?;

        let mut players: Vec<EasyTierMember> = vec![];
        for item in object.as_array()? {
            let hostname = item.as_object()?.get("hostname")?.as_str()?.to_string();
            let address = Ipv4Addr::from_str(item.as_object()?.get("ipv4")?.as_str()?).ok();
            let is_local = item.as_object()?.get("cost")?.as_str()? == "Local";
            let nat = match item.as_object()?.get("nat_type")?.as_str()? {
                "Unknown" => NatType::Unknown,
                "OpenInternet" => NatType::OpenInternet,
                "NoPat" => NatType::NoPAT ,
                "FullCone" => NatType::FullCone,
                "Restricted" => NatType::Restricted,
                "PortRestricted" => NatType::PortRestricted,
                "Symmetric" => NatType::Symmetric,
                "SymUdpFirewall" => NatType::SymmetricUdpWall,
                "SymmetricEasyInc" => NatType::SymmetricEasyIncrease,
                "SymmetricEasyDec" => NatType::SymmetricEasyDecrease,
                #[cfg(debug_assertions)]
                nat => panic!("Unknown NAT type: {}", nat),
                #[cfg(not(debug_assertions))]
                _ => return None,
            };

            players.push(EasyTierMember { hostname, address, is_local, nat });
        }
        Some(players)
    }

    pub fn add_port_forward(&mut self, forwards: &[PortForward]) -> bool {
        let mut processes: Vec<(&PortForward, Option<Child>)> = forwards.iter().map(|forward| (forward, None)).collect();

        for time in 0..3 {
            for (PortForward { local, remote, proto }, process_holder) in processes.iter_mut() {
                let mut process = match self.start_cli().args([
                    "-p", &format!("127.0.0.1:{}", self.rpc), "port-forward", "add",
                    proto.name(), &local.to_string(), &remote.to_string(),
                ]).spawn() {
                    Ok(v) => v,
                    Err(e) => {
                        logging!("EasyTier CLI", "Cannot spawn easytier cli instance: {:?}", e);
                        return false;
                    }
                };
                forward_std(&mut process, |line| {
                    logging!("EasyTier CLI", "{}", line);
                });

                process_holder.replace(process);
            }

            for i in (0..processes.len()).rev() {
                if processes[i].1.as_mut().unwrap().wait().is_ok_and(|status| status.success()) {
                    processes.swap_remove(i);
                }
            }

            if processes.is_empty() {
                return true;
            }

            thread::sleep(Duration::from_millis(time * 1000 + 500))
        }

        if !processes.is_empty() {
            let mut msg = "Cannot adding port-forward rules: ".to_string();
            for (i, (PortForward { local, remote, proto }, _)) in processes.iter().enumerate() {
                write!(&mut msg, "{} -> {} ({})", local, remote, proto.name()).unwrap();
                if i != processes.len() - 1 {
                    msg.push_str(", ");
                }
            }
            logging!("EasyTier CLI", "{}", msg);
            return false;
        }
        true
    }

    fn start_cli(&self) -> Command {
        let mut command = Command::new(FACTORY.cli.as_path());
        command.current_dir(env::temp_dir())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }
}

impl Drop for EasyTier {
    fn drop(&mut self) {
        logging!("EasyTier", "Killing EasyTier.");
        let _ = self.process.lock().kill();
    }
}
