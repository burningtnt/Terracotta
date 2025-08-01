use std::io::Result;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

pub struct FakeServer {
    pub port: u16,
    signal: Sender<Signals>,
}

impl FakeServer {
    pub fn activate(&self) {
        let _ = self.signal.send(Signals::Activate);
    }
}

enum Signals {
    Activate, Terminate
}

pub fn create(port: u16, motd: &'static str) -> FakeServer {
    let (tx, rx) = mpsc::channel::<Signals>();
    thread::spawn(move || run(port, motd, rx));

    return FakeServer { port: port, signal: tx };
}

fn run(port: u16, motd: &'static str, signal: Receiver<Signals>) {
    let sockets: Vec<(UdpSocket, &'static SocketAddr)> = crate::ADDRESSES
        .iter()
        .map(|address| {
            let socket = UdpSocket::bind((address.clone(), 0))?;
            let ip: &SocketAddr = match address {
                IpAddr::V4(_) => {
                    socket.set_broadcast(true)?;
                    socket.set_multicast_ttl_v4(4)?;
                    socket.set_multicast_loop_v4(true)?;

                    lazy_static::lazy_static! {
                        static ref ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 2, 60)), 4445);
                    }

                    &*ADDR
                }
                IpAddr::V6(_) => {
                    socket.set_multicast_loop_v6(true)?;

                    lazy_static::lazy_static! {
                        static ref ADDR: SocketAddr = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0xFF75, 0x230, 0, 0, 0, 0, 0, 0x60)), 4445);
                    }

                    &*ADDR
                }
            };

            return Ok((socket, ip));
        })
        .filter_map(|r: Result<(UdpSocket, &SocketAddr)>| match r {
            Ok(value) => Some(value), 
            Err(_) => None
        })
        .collect();

    match signal.recv().unwrap() {
        Signals::Activate => {
            let message: String = format!("[MOTD]{}[/MOTD][AD]{}[/AD]", motd, port);
            let message_bytes = message.as_bytes();

            loop {
                if let Ok(signal) = signal.try_recv() && let Signals::Terminate = signal {
                    break;
                }

                for (socket, address) in sockets.iter() {
                    let _ = socket.send_to(message_bytes, address);
                }

                thread::sleep(Duration::from_millis(1500));
            }
        },
        Signals::Terminate => {
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        let _ = self.signal.send(Signals::Terminate);
    }
}
