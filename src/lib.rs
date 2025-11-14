#![feature(panic_backtrace_config, const_convert, const_trait_impl, unsafe_cell_access, panic_update_hook, internal_output_capture, string_from_utf8_lossy_owned)]

extern crate core;

#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        crate::logging_android(std::format!("[{}]: {}", $prefix, std::format_args!($($arg)*)));
    };
}

use lazy_static::lazy_static;

use chrono::{FixedOffset, TimeZone, Utc};
use jni::{JNIEnv, objects::JString, strings::JavaStr, sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jobject}};
use std::{
    env, ffi::CString, fs, net::{IpAddr, Ipv4Addr, Ipv6Addr}, ptr::null_mut, sync::{Arc, Mutex}, thread
};
use libc::{c_char, c_int};

use crate::controller::Room;

pub mod controller;
mod easytier;
mod scaffolding;
pub const MOTD: &'static str = "§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）";

mod mc;
mod ports;

type JNIRawEnv = *mut jni::sys::JNIEnv;

lazy_static::lazy_static! {
    static ref ADDRESSES: Vec<IpAddr> = {
        let mut addresses: Vec<IpAddr> = vec![];

        if let Ok(networks) = local_ip_address::list_afinet_netifas() {
            logging!("UI", "Local IP Addresses: {:?}", networks);

            for (_, address) in networks.into_iter() {
                match address {
                    IpAddr::V4(ip) => {
                        let parts = ip.octets();
                        if !(parts[0] == 10 && parts[1] == 144 && parts[2] == 144) && ip != Ipv4Addr::LOCALHOST && ip != Ipv4Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    },
                    IpAddr::V6(ip) => {
                        if ip != Ipv6Addr::LOCALHOST && ip != Ipv6Addr::UNSPECIFIED {
                            addresses.push(address);
                        }
                    }
                };
            }
        }

        addresses.push(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        addresses.push(IpAddr::V6(Ipv6Addr::UNSPECIFIED));

        addresses.sort_by(|ip1, ip2| ip2.cmp(ip1));
        addresses
    };
}

lazy_static! {
    static ref FILE_ROOT: std::path::PathBuf = {
        let path = if cfg!(target_os = "macos")
            && let Ok(home) = env::var("HOME")
        {
            std::path::Path::new(&home).join("terracotta")
        } else {
            std::path::Path::new(&env::temp_dir()).join("terracotta")
        };

        fs::create_dir_all(&path).unwrap();

        path
    };
    static ref MACHINE_ID_FILE: std::path::PathBuf = FILE_ROOT.join("machine-id");
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_start(_env: JNIRawEnv) -> jint {
    cfg_if::cfg_if! {
        if #[cfg(debug_assertions)] {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Short);
        } else {
            std::panic::set_backtrace_style(std::panic::BacktraceStyle::Full);
        }
    }

    std::panic::update_hook(|prev, info| {
        let data = Arc::new(Mutex::new(Vec::<u8>::new()));
        std::io::set_output_capture(Some(data.clone()));
        prev(info);
        std::io::set_output_capture(None);

        let data = match Arc::try_unwrap(data) {
            Ok(data) => String::from_utf8_lossy_owned(data.into_inner().unwrap()),
            Err(data) => String::from_utf8_lossy_owned(data.lock().unwrap().clone()) // Should NOT happen.
        };
        logging_android(data);
    });

    logging!(
        "UI",
        "Welcome using Terracotta v{}, compiled at {}. Easytier: {}. Target: {}-{}-{}-{}.",
        env!("TERRACOTTA_VERSION"),
        Utc.timestamp_millis_opt(timestamp::compile_time!() as i64)
            .unwrap()
            .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M:%S"),
        env!("TERRACOTTA_ET_VERSION"),
        env!("CARGO_CFG_TARGET_ARCH"),
        env!("CARGO_CFG_TARGET_VENDOR"),
        env!("CARGO_CFG_TARGET_OS"),
        env!("CARGO_CFG_TARGET_ENV"),
    );

    if let Err(e) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build() {
        logging!("UI", "Cannot launch tokio runtime: {:?}", e);
        return 2;
    }

    thread::spawn(|| {
        lazy_static::initialize(&controller::SCAFFOLDING_PORT);
        lazy_static::initialize(&easytier::FACTORY);
    });

    return 0;
}

fn logging_android(line: String) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    let line = CString::new(line).unwrap();

    // SAFETY: 4 is ANDROID_LOG_INFO, while pointers to tag and line are valid.
    unsafe {
        __android_log_write(4, c"hello".as_ptr(), line.as_ptr());
    }
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_getState(env: JNIRawEnv) -> jobject {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    env.new_string(serde_json::to_string(&controller::get_state()).unwrap()).unwrap().into_raw()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setWaiting(_env: JNIRawEnv) {
    controller::set_waiting();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setScanning(env: JNIRawEnv, player: jobject) {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    let player  = parse_jstring(&env, player);

    controller::set_scanning(player);
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setGuesting(env: JNIRawEnv, room: jobject, player: jobject) -> jboolean {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    let room  = parse_jstring(&env, room);
    let player  = parse_jstring(&env, player);

    if let Some(room) = room && let Some(room) = Room::from(&room) && controller::set_guesting(room, player) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

fn parse_jstring(env: &JNIEnv<'static>, value: jobject) -> Option<String> {
    if value == null_mut() {
        None
    } else {
        // SAFETY: value is a Java String Object
        
        let value = unsafe { JString::from_raw(value) };
        Some(<JavaStr<'_, '_, '_> as Into<String>>::into(unsafe { 
            env.get_string_unchecked(&value) 
        }.unwrap().into()))
    }
}