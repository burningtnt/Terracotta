use crate::controller::Room;

pub type PublicServers = Vec<String>;

pub fn fetch_public_nodes(_: &Room) -> PublicServers {
    vec![
        "tcp://public.easytier.top:11010".into(),
        "tcp://public2.easytier.cn:54321".into()
    ]
}
