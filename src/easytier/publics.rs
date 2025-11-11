use std::io;
use std::time::Duration;
use rand_chacha::ChaCha12Rng;
use rand_core::{RngCore, SeedableRng};
use serde_json::Value;
use crate::controller::{Room, RoomKind};

pub type PublicServers = Vec<String>;

pub fn fetch_public_nodes(room: &Room) -> PublicServers {
    static FALLBACK_SERVERS: [&str; 2] = [
        "tcp://public.easytier.top:11010",
        "tcp://public2.easytier.cn:54321",
    ];

    fn fetch_inner(room: &Room) -> io::Result<Vec<String>> {
        static LIMIT: usize = 5;

        let mut servers: Vec<String> = serde_json::from_reader::<_, Value>(
            reqwest::blocking::Client::builder()
                .user_agent(format!("Terracotta v{}/s={}", env!("TERRACOTTA_ET_VERSION"), option_env!("TERRACOTTA_U_S").unwrap_or("SNAPSHOT")))
                .timeout(Some(Duration::from_secs(10)))
                .build()
                .map_err(io::Error::other)?
                .get("https://uptime.easytier.cn/api/nodes?is_active=true&page=1&per_page=100&&tags=MC%E4%B8%AD%E7%BB%A7")
                .send()
                .map_err(io::Error::other)?
        ).map_err(io::Error::other)?
            .as_object()
            .and_then(|object| {
                if !object.get("success")?.as_bool()? {
                    return None;
                }

                Some(object.get("data")?.as_object()?
                    .get("items")?.as_array()?
                    .iter()
                    .filter_map(|node| node.as_object())
                    .flat_map(|node| {
                        let address = node.get("address")?.as_str()?;
                        if node.get("allow_relay")?.as_bool()? && node.get("is_active")?.as_bool()? && !FALLBACK_SERVERS.contains(&address) {
                            Some(address.to_string())
                        } else {
                            None
                        }
                    })
                    .collect())
            })
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;

        if servers.len() > LIMIT {
            let mut rng = ChaCha12Rng::from_seed(match room.kind {
                RoomKind::Experimental { seed } => {
                    let mut value = [0u8; 32];
                    value[0..16].copy_from_slice(&seed.to_be_bytes());
                    value
                }
                _ => unreachable!(),
            });

            for i in (1..servers.len()).rev() {
                servers.swap(i, rng.next_u32() as usize % (i + 1));
            }
            servers.truncate(5);
        }
        for fallback in FALLBACK_SERVERS {
            servers.push(fallback.to_string());
        }
        Ok(servers)
    }

    fetch_inner(room).unwrap_or_else(|e| {
        logging!("RoomExperiment", "Cannot fetch EasyTier public nodes: {:?}.", e);
        FALLBACK_SERVERS.map(|s| s.into()).to_vec()
    })
}