use std::net::{Ipv4Addr, TcpListener};

#[repr(u8)]
pub enum PortRequest {
    EasyTierRPC,
    Scaffolding,
    Minecraft
}

impl PortRequest {
    pub fn request(self) -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|socket| socket.local_addr())
            .map(|address| address.port())
            .unwrap_or(self as u8 as u16 + 35780)
    }
}