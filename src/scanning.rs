use std::borrow::Cow;
use std::io::Result;
use std::net::{Ipv4Addr, Ipv6Addr, UdpSocket};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SIG_TERMINAL: u8 = 1;

pub struct Scanning {
    signal: Sender<u8>,
    port: Arc<Mutex<Vec<u16>>>,
}

fn set_socket_reuse_address(socket: &UdpSocket) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            let optval: libc::c_int = 1;
            let ret = libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEPORT,
                &optval as *const _ as *const libc::c_void,
                std::mem::size_of_val(&optval) as libc::socklen_t,
            );
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }

    return Ok(());
}

impl Scanning {
    pub fn create(filter: fn(&str) -> bool) -> Scanning {
        let (tx, rx): (Sender<u8>, Receiver<u8>) = mpsc::channel();
        let port = Arc::new(Mutex::new(vec![]));

        let port_cloned = Arc::clone(&port);
        thread::spawn(move || {
            let result = Self::run(rx, port_cloned, filter);

            match result {
                Ok(_) => {}
                Err(err) => {
                    logging!("Server Scanner", "Cannot scan: {}", err);
                }
            }
        });

        return Scanning {
            signal: tx,
            port: port,
        };
    }

    fn run(
        signal: Receiver<u8>,
        output: Arc<Mutex<Vec<u16>>>,
        filter: fn(&str) -> bool,
    ) -> Result<()> {
        let mut sockets: Vec<UdpSocket> = vec![{
            let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 4445))?;
            set_socket_reuse_address(&socket)?;

            socket.join_multicast_v6(&Ipv6Addr::from_str("FF75:230::60").unwrap(), 0)?;
            socket.set_read_timeout(Some(Duration::from_millis(500)))?;

            socket
        }];

        if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 4445)) {
            set_socket_reuse_address(&socket)?;

            socket.join_multicast_v4(
                &Ipv4Addr::from_str("224.0.2.60").unwrap(),
                &Ipv4Addr::UNSPECIFIED,
            )?;
            socket.set_read_timeout(Some(Duration::from_millis(500)))?;

            sockets.push(socket);
        }

        let mut buf: [u8; 8192] = [0; 8192];

        let mut ports: Vec<(u16, Instant)> = vec![];

        loop {
            let mut dirty = false;

            if let Ok(value) = signal.recv_timeout(Duration::from_millis(500)) {
                if value == SIG_TERMINAL {
                    return Ok(());
                } else {
                    panic!("Unknown signal {}.", value)
                }
            }

            let now = Instant::now();
            for i in (0..ports.len()).rev() {
                if match now.checked_duration_since(ports[i].1) {
                    Some(value) => value.as_millis() >= 5_000,
                    None => false,
                } {
                    dirty = true;
                    ports.remove(i);
                }
            }

            for socket in sockets.iter() {
                if let Ok((length, _sender)) = socket.recv_from(&mut buf) {
                    let data: Cow<'_, str> = String::from_utf8_lossy(&buf[..length]);

                    {
                        let begin = data.find("[MOTD]");
                        let end = data.find("[/MOTD]");
                        if let Some(begin) = begin
                            && let Some(end) = end
                            && end - begin >= "[MOTD]".len() + 1
                            && let Some(motd) = data.as_ref().get((begin + "[MOTD]".len())..end)
                            && filter(motd)
                        {
                        } else {
                            continue;
                        }
                    }

                    {
                        let begin = data.find("[AD]");
                        let end = data.find("[/AD]");
                        if let Some(begin) = begin
                            && let Some(end) = end
                            && end - begin >= "[AD]".len() + 1
                            && let Some(port) = data.as_ref().get((begin + "[AD]".len())..end)
                            && let Ok(port) = port.parse::<u16>()
                        {
                            let mut existed = false;
                            for i in 0..ports.len() {
                                if ports[i].0 == port {
                                    existed = true;
                                    ports.remove(i);
                                    break;
                                }
                            }

                            ports.push((port, Instant::now()));
                            if !existed {
                                dirty = true;
                            }
                            break;
                        }
                    }
                }
            }

            if dirty {
                let mut output = output.lock().unwrap();
                output.clear();

                let mut message = String::from("Updating server list to [");
                for i in 0..ports.len() {
                    output.push(ports[0].0);
                    message += &ports[0].0.to_string();
                    if i != ports.len() - 1 {
                        message.push_str(", ");
                    }
                }
                message.push_str("]");
                logging!("Server Scanner", "{}", message);
            }
        }
    }

    pub fn get_ports(&self) -> Vec<u16> {
        let mut vec: Vec<u16> = vec![];
        for port in self.port.lock().unwrap().iter() {
            vec.push(*port);
        }
        return vec;
    }
}

impl Drop for Scanning {
    fn drop(&mut self) {
        let _ = self.signal.send(SIG_TERMINAL);
    }
}
