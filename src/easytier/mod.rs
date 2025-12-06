use crate::controller::ConnectionDifficulty;
use crate::easytier::argument::{Argument, PortForward};
use cfg_if::cfg_if;
use std::cmp::PartialEq;
use std::net::Ipv4Addr;

pub mod argument;
pub mod publics;

cfg_if! {
    if #[cfg(not(target_os = "android"))] {
        mod executable_impl;
        use executable_impl as inner;
    }
}

pub struct EasyTier(inner::EasyTier);

#[derive(Debug)]
pub struct EasyTierMember {
    pub hostname: String,
    pub address: Option<Ipv4Addr>,
    pub is_local: bool,
    pub nat: NatType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NatType {
    Unknown,
    OpenInternet,
    NoPAT,
    FullCone,
    Restricted,
    PortRestricted,
    Symmetric,
    SymmetricUdpWall,
    SymmetricEasyIncrease,
    SymmetricEasyDecrease,
}

pub fn calc_conn_difficulty(left: &NatType, right: &NatType) -> ConnectionDifficulty {
    let is = |types: &[NatType]| -> bool {
        types.contains(left) || types.contains(right)
    };

    if is(&[NatType::OpenInternet]) {
        ConnectionDifficulty::Easiest
    } else if is(&[NatType::NoPAT,NatType::FullCone]) {
        ConnectionDifficulty::Simple
    } else if is(&[NatType::Restricted, NatType::PortRestricted]) {
        ConnectionDifficulty::Medium
    } else {
        ConnectionDifficulty::Tough
    }
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