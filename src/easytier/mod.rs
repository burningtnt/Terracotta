pub mod argument;
pub mod publics;

#[cfg(not(target_os = "android"))]
mod executable_impl;
#[cfg(not(target_os = "android"))]
pub use executable_impl::*;