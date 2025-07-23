use std::borrow::Cow;
use std::io::Result;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{mem, thread};

use socket2::{Domain, SockAddr, Socket, Type};

const SIG_TERMINAL: u8 = 1;

pub struct Scanning {
    signal: Sender<u8>,
    port: Arc<Mutex<Vec<u16>>>,
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
        let sockets: Vec<(Socket, &IpAddr)> = crate::ADDRESSES
            .iter()
            .map(|address| match address {
                IpAddr::V4(ip) => {
                    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
                    socket.set_reuse_address(true)?;
                    socket.bind(&SockAddr::from(SocketAddrV4::new(ip.clone(), 4445)))?;
                    socket.join_multicast_v4(
                        &Ipv4Addr::from_str("224.0.2.60").unwrap(),
                        &Ipv4Addr::UNSPECIFIED,
                    )?;
                    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
                    Ok((socket, address))
                }
                IpAddr::V6(ip) => {
                    let socket = Socket::new(Domain::IPV6, Type::DGRAM, None)?;
                    socket.set_only_v6(true)?;
                    socket.set_reuse_address(true)?;
                    socket.bind(&SockAddr::from(SocketAddrV6::new(ip.clone(), 4445, 0, 0)))?;
                    socket.join_multicast_v6(&Ipv6Addr::from_str("FF75:230::60").unwrap(), 0)?;
                    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
                    Ok((socket, address))
                }
            })
            .filter_map(|r: Result<(Socket, &IpAddr)>| match r {
                Ok(value) => Some(value),
                Err(_) => None,
            })
            .collect();

        logging!("Server Scanner", "Starting server scanner at IP: {:?}", sockets.iter().map(|p| p.1).collect::<Vec<_>>());

        let mut buf: [mem::MaybeUninit<u8>; 8192] =
            unsafe { mem::MaybeUninit::uninit().assume_init() };

        let mut ports: Vec<(u16, SystemTime)> = vec![];

        loop {
            let mut dirty = false;

            if let Ok(value) = signal.recv_timeout(Duration::from_millis(500)) {
                if value == SIG_TERMINAL {
                    return Ok(());
                } else {
                    panic!("Unknown signal {}.", value)
                }
            }

            let now = SystemTime::now();
            for i in (0..ports.len()).rev() {
                if match now.duration_since(ports[i].1) {
                    Ok(value) => value.as_millis() >= 5_000,
                    Err(_) => false,
                } {
                    dirty = true;
                    ports.remove(i);
                }
            }

            for (socket, _) in sockets.iter() {
                if let Ok((length, _)) = socket.recv_from(&mut buf) {
                    let buf = unsafe { mem::transmute::<_, &[u8]>(&buf[..length]) };

                    let data: Cow<'_, str> = String::from_utf8_lossy(buf);
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

                            ports.push((port, SystemTime::now()));
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
