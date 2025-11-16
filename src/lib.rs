#![feature(
    panic_backtrace_config,
    const_convert,
    const_trait_impl,
    unsafe_cell_access,
    panic_update_hook,
    internal_output_capture,
    string_from_utf8_lossy_owned
)]

extern crate core;
#[cfg(not(target_os = "android"))]
compile_error!("Terracotta lib mode is intended for Android platform.");

#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        crate::logging_android(std::format!("[{}]: {}", $prefix, std::format_args!($($arg)*)));
    };
}

use lazy_static::lazy_static;

use crate::controller::Room;
use chrono::{FixedOffset, TimeZone, Utc};
use jni::objects::JClass;
use jni::signature::{Primitive, ReturnType};
use jni::sys::{jclass, jshort, jsize, jvalue, JavaVM};
use jni::{objects::JString, sys::{jboolean, jint, jobject, JNI_FALSE, JNI_TRUE}, JNIEnv};
use libc::{c_char, c_int};
use std::time::Duration;
use std::{
    env, ffi::CString, fs, net::{IpAddr, Ipv4Addr, Ipv6Addr}, sync::{Arc, Mutex}, thread,
};

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

static VPN_SERVICE_CFG: Mutex<Option<crate::easytier::EasyTierTunRequest>> = Mutex::new(None);

// FIXME: Third-party crate 'jni-sys' leaves a dynamic link to JNI_GetCreatedJavaVMs,
// which doesn't exist on Android, so A dummy JNI_GetCreatedJavaVMs is declared as a workaround.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn JNI_GetCreatedJavaVMs(_: *mut *mut JavaVM, _: jsize, _: *mut jsize) -> jint {
    unreachable!();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_start0(env: JNIRawEnv, clazz: jclass) -> jint {
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

    let jenv = unsafe { JNIEnv::from_raw(env) }.unwrap();
    let jvm = jenv.get_java_vm().unwrap();
    let clazz = jenv.new_global_ref(unsafe  { JClass::from_raw(clazz) }).unwrap();

    thread::spawn(move || {
        let mut jenv = jvm.attach_current_thread_as_daemon().unwrap();

        let on_vpn_service_sc = jenv.get_static_method_id(
            &clazz, "onVpnServiceStateChanged", "(BBBBSLjava/lang/String;)I"
        ).unwrap();

        loop {
            thread::sleep(Duration::from_millis(1000));

            let Some(cfg) = ({
                VPN_SERVICE_CFG.lock().unwrap().take()
            }) else {
                continue;
            };

            let [ip1, ip2, ip3, ip4] = cfg.address.octets().map(|i| i as i8);
            let cidrs = cfg.cidrs.join("\0");
            let cidrs2 = jenv.new_string(cidrs).unwrap();

            let tun_fd = unsafe {
                jenv.call_static_method_unchecked(&clazz, on_vpn_service_sc, ReturnType::Primitive(Primitive::Int), &[
                    jvalue { b: ip1 }, jvalue { b: ip2 }, jvalue { b: ip3 }, jvalue { b: ip4 },
                    jvalue { s: cfg.network_length as jshort },
                    jvalue { l: cidrs2.into_raw() }
                ])
            }.unwrap();
            if !jenv.exception_check().unwrap() {
                cfg.dest.write().unwrap().replace(tun_fd.i().unwrap());
            }
        }
    });

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
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_getState0(env: JNIRawEnv, _: jclass) -> jobject {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    env.new_string(serde_json::to_string(&controller::get_state()).unwrap()).unwrap().into_raw()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setWaiting0(_env: JNIRawEnv, _: jclass) {
    controller::set_waiting();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setScanning0(env: JNIRawEnv, _: jclass, player: jobject) {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    let player = parse_jstring(&env, player);

    controller::set_scanning(player);
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn Java_net_burningtnt_terracotta_TerracottaAndroidAPI_setGuesting0(env: JNIRawEnv, _: jclass, room: jobject, player: jobject) -> jboolean {
    let env = unsafe { JNIEnv::from_raw(env) }.unwrap();
    let room = parse_jstring(&env, room);
    let player = parse_jstring(&env, player);

    if let Some(room) = room && let Some(room) = Room::from(&room) && controller::set_guesting(room, player) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

pub(crate) fn on_vpnservice_change(request: crate::easytier::EasyTierTunRequest) {
    let mut guard = VPN_SERVICE_CFG.lock().unwrap();
    *guard = Some(request);
}

fn parse_jstring(env: &JNIEnv<'static>, value: jobject) -> Option<String> {
    if value.is_null() {
        None
    } else {
        // SAFETY: value is a Java String Object
        unsafe {
            Some(env.get_string_unchecked(&JString::from_raw(value)).unwrap().into())
        }
    }
}