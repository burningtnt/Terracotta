use crate::controller::Room;

pub type PublicServers = Vec<String>;

pub fn fetch_public_nodes(_: &Room) -> PublicServers {
    Vec::from([
        "tcp://public.easytier.top:11010",
        "tcp://public2.easytier.cn:54321",
        "https://etnode.zkitefly.eu.org/node1",
        "https://etnode.zkitefly.eu.org/node2",
    ].map(|s| s.into()))
}
