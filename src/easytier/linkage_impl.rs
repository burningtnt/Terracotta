use std::cell::UnsafeCell;
use crate::easytier::argument::{Argument, PortForward};
use easytier::common::config::{PortForwardConfig, TomlConfigLoader};
use easytier::launcher::NetworkInstance;
use easytier::proto::api::instance::{ListRouteRequest, Route};
use easytier::proto::rpc_types::controller::BaseController;
use easytier::socks5::Socks5Server;
use std::fmt::Write;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::runtime::Handle;
use toml::{Table, Value};

lazy_static::lazy_static! {
    pub static ref FACTORY: EasytierFactory = create();
}

pub struct EasytierFactory();

pub struct Easytier {
    instance: Option<(NetworkInstance, Arc<Socks5Server>)>,
}

fn create() -> EasytierFactory {
    EasytierFactory()
}

impl EasytierFactory {
    pub fn create(&self, args: Vec<Argument>) -> Easytier {
        let table = UnsafeCell::new(Table::new());
        let acquire_table = || {
            unsafe {
                table.as_mut_unchecked()
            }
        };

        acquire_table().insert("flags".into(), Value::Table(Table::new()));
        let flags = || acquire_table().get_mut("flags").unwrap().as_table_mut().unwrap();

        acquire_table().insert("network_identity".into(), Value::Table(Table::new()));
        let identity = || acquire_table().get_mut("network_identity").unwrap().as_table_mut().unwrap();

        acquire_table().insert("listeners".into(), Value::Array(vec![]));
        let listeners = || acquire_table().get_mut("listeners").unwrap().as_array_mut().unwrap();

        acquire_table().insert("peer".into(), Value::Array(vec![]));
        let peer = || acquire_table().get_mut("peer").unwrap().as_array_mut().unwrap();

        acquire_table().insert("port_forward".into(), Value::Array(vec![]));
        let forwards = || acquire_table().get_mut("port_forward").unwrap().as_array_mut().unwrap();

        acquire_table().insert("tcp_whitelist".into(), Value::Array(vec![]));
        let tcp_whitelist = || acquire_table().get_mut("tcp_whitelist").unwrap().as_array_mut().unwrap();

        acquire_table().insert("udp_whitelist".into(), Value::Array(vec![]));
        let udp_whitelist = || acquire_table().get_mut("udp_whitelist").unwrap().as_array_mut().unwrap();

        for arg in args {
            match arg {
                Argument::NoTun => {
                    flags().insert("no_tun".into(), Value::Boolean(true));
                }
                Argument::Compression(name) => {
                    flags().insert("data_compress_algo".into(), Value::Integer(match name.as_ref() {
                        "zstd" => 2,
                        _ => unimplemented!(),
                    }));
                }
                Argument::MultiThread => {
                    flags().insert("multi_thread".into(), Value::Boolean(true));
                }
                Argument::LatencyFirst => {
                    flags().insert("latency_first".into(), Value::Boolean(true));
                }
                Argument::EnableKcpProxy => {
                    flags().insert("enable_kcp_proxy".into(), Value::Boolean(true));
                }
                Argument::PublicServer(server) => {
                    peer().push(Value::String(server.into()));
                }
                Argument::NetworkName(name) => {
                    identity().insert("network_name".into(), Value::String(name.into()));
                }
                Argument::NetworkSecret(secret) => {
                    identity().insert("network_secret".into(), Value::String(secret.into()));
                }
                Argument::Listener { address, proto } => {
                    listeners().push(Value::String(format!("{}://{}", proto.name(), address)));
                }
                Argument::PortForward(PortForward { local, remote, proto }) => {
                    let mut forward = Table::new();
                    forward.insert("bind_addr".into(), Value::String(local.to_string()));
                    forward.insert("dst_addr".into(), Value::String(remote.to_string()));
                    forward.insert("proto".into(), Value::String(proto.name().into()));
                    forwards().push(Value::Table(forward));
                }
                Argument::DHCP => {
                    acquire_table().insert("dhcp".into(), Value::Boolean(true));
                }
                Argument::HostName(name) => {
                    acquire_table().insert("hostname".into(), Value::String(name.into()));
                }
                Argument::IPv4(address) => {
                    acquire_table().insert("ipv4".into(), Value::String(address.to_string()));
                }
                Argument::TcpWhitelist(port) => {
                    tcp_whitelist().push(Value::Integer(port as i64));
                }
                Argument::UdpWhitelist(port) => {
                    udp_whitelist().push(Value::Integer(port as i64));
                }
            }
        }

        let instance = toml::to_string(&Value::Table(table.into_inner())).ok()
            .and_then(|str| TomlConfigLoader::new_from_str(str.as_str()).ok())
            .map(|config| NetworkInstance::new(config));
        let instance = if let Some(mut instance) = instance && let Ok((_, server)) = instance.start() {
            Some((instance, server.unwrap()))
        } else {
            None
        };
        Easytier { instance }
    }

    pub fn remove(&self) {}
}

impl Easytier {
    pub fn is_alive(&mut self) -> bool {
        self.instance.as_ref().is_some_and(|(instance, _)| instance.is_easytier_running())
    }

    pub fn get_players(&mut self) -> Option<Vec<(String, Ipv4Addr)>> {
        self.instance.as_ref()
            .and_then(|(instance, _)| {
                instance.get_api_service()
                    .and_then(|service| {
                        Handle::current().block_on(service.get_peer_manage_service()
                            .list_route(BaseController::default(), ListRouteRequest::default())
                        ).ok()
                    })
                    .map(|response| response.routes)
            })
            .map(|info: Vec<Route>| {
                info.into_iter()
                    .filter_map(|route| route.ipv4_addr
                        .and_then(|address| address.address)
                        .map(|address| (route.hostname, Ipv4Addr::from_octets(address.addr.to_be_bytes())))
                    )
                    .collect::<Vec<_>>()
            })
    }

    pub fn add_port_forward(
        &mut self,
        forwards: &[PortForward],
    ) -> bool {
        if let Some((_, socks5)) = self.instance.as_ref() {
            let mut stream = forwards.iter().map(|forward| {
                let task = socks5.add_port_forward(PortForwardConfig {
                    bind_addr: forward.local,
                    dst_addr: forward.remote,
                    proto: forward.proto.name().into(),
                });

                (task, forward)
            }).filter_map(|(task, forward)| {
                Handle::current().block_on(task).err().map(|e| (e, forward))
            });

            if let Some(mut item) = stream.next() {
                let mut msg = "Cannot adding port-forward rules: ".to_string();
                loop {
                    let (e, PortForward { local, remote, proto }) = item;
                    write!(&mut msg, "{} -> {} ({}): {:?}", local, remote, proto.name(), e).unwrap();

                    if let Some(item2) = stream.next() {
                        msg.push_str(", ");
                        item = item2;
                    } else {
                        break;
                    }
                }
                logging!("EasyTier CLI", "{}", msg);
            } else {
                return true;
            }
        }
        return false;
    }
}

impl Drop for Easytier {
    fn drop(&mut self) {
        logging!("EasyTier", "Killing EasyTier.");

        self.instance.take()
            .and_then(|(instance, _)| instance.get_stop_notifier())
            .map(|stop| {
                Handle::current().block_on(stop.notified());
            });
    }
}
