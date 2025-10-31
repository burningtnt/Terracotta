use std::borrow::Cow;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

type CowString = Cow<'static, str>;

#[derive(Clone)]
pub enum Proto {
    TCP, UDP
}

impl Proto {
    pub fn name(&self) -> &'static str {
        match self {
            Proto::TCP => "tcp",
            Proto::UDP => "udp"
        }
    }
}

#[derive(Clone)]
pub enum Argument {
    NoTun,
    Compression(CowString),
    MultiThread,
    LatencyFirst,
    EnableKcpProxy,
    NetworkName(CowString),
    NetworkSecret(CowString),
    PublicServer(CowString),
    Listener {
        address: SocketAddr,
        proto: Proto
    },
    PortForward {
        local: SocketAddr,
        remote: SocketAddr,
        proto: Proto,
    },
    DHCP,
    HostName(CowString),
    IPv4(Ipv4Addr),
    TcpWhitelist(u16),
    UdpWhitelist(u16),
}