use std::borrow::Cow;
use std::net::{Ipv4Addr, SocketAddr};

type CowString = Cow<'static, str>;

#[derive(Clone)]
pub struct PortForward {
    pub(crate) local: SocketAddr,
    pub(crate) remote: SocketAddr,
    pub(crate) proto: Proto,
}

#[derive(Clone)]
pub enum Proto {
    TCP,
    UDP,
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
        proto: Proto,
    },
    PortForward(PortForward),
    DHCP,
    HostName(CowString),
    IPv4(Ipv4Addr),
    TcpWhitelist(u16),
    UdpWhitelist(u16),
}