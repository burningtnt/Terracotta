use std::net::Ipv4Addr;
use crate::easytier::argument::{Argument, PortForward};
use cfg_if::cfg_if;

pub mod argument;
pub mod publics;

cfg_if! {
    if #[cfg(not(target_os = "android"))] {
        mod executable_impl;
        use executable_impl as inner;
    }
}

pub struct EasyTier(inner::EasyTier);

pub struct EasyTierMember {
    pub hostname: String,
    pub address: Ipv4Addr
}

pub fn create(args: Vec<Argument>) -> EasyTier {
    EasyTier(inner::create(args))
}

pub fn initialize() {
    inner::initialize();
}

pub fn cleanup() {
    inner::cleanup();
}

impl EasyTier {
    pub fn is_alive(&self) -> bool {
        self.0.is_alive()
    }

    pub fn get_players(&self) -> Option<Vec<EasyTierMember>> {
        self.0.get_players()
    }

    pub fn add_port_forward(&mut self, forwards: &[PortForward]) -> bool {
        self.0.add_port_forward(forwards)
    }
}