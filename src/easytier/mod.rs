#[cfg(not(target_os = "android"))]
mod executable_impl;
pub mod argument;

#[cfg(not(target_os = "android"))]
pub use executable_impl::*;