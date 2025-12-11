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
compile_error!("Terracotta Library is intended for Android platform.");

#[macro_export]
macro_rules! logging {
    ($prefix:expr, $($arg:tt)*) => {
        crate::logging_android(std::format!("[{}]: {}", $prefix, std::format_args!($($arg)*)));
    };
}

macro_rules! try_jvm {
    (|$jenv:ident| $($tokens:tt)*) => {{
        let mut $jenv = $jenv;

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            $($tokens)*
        })) {
            Ok(val) => val,
            Err(e) => {
                let _ = $jenv.exception_clear();
                $jenv.throw(("java/lang/RuntimeException", format!("Terracotta panics: {:?}", e))).unwrap();
                unsafe { std::mem::zeroed() }
            }
        }
    }};
}

use crate::controller::{Room, RoomKind};
use crate::once_cell::OnceCell;
use chrono::{FixedOffset, TimeZone, Utc};
use jni::signature::{Primitive, ReturnType};
use jni::sys::JNI_VERSION_1_6;
use jni::{objects::{JClass, JString}, sys::{jboolean, jint, jlong, jshort, jsize, jvalue, JNI_FALSE, JNI_TRUE}, JNIEnv, JavaVM, NativeMethod};
use libc::{c_char, c_int};
use std::ffi::c_void;
use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::Duration;
use std::{
    env, ffi::CString, net::{IpAddr, Ipv4Addr, Ipv6Addr}, sync::{Arc, Mutex}, thread,
};

pub mod controller;
mod easytier;
mod scaffolding;
pub const MOTD: &'static str = "§6§l双击进入陶瓦联机大厅（请保持陶瓦运行）";

mod mc;
mod ports;
mod once_cell;

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

static MACHINE_ID_FILE: OnceCell<std::path::PathBuf> = OnceCell::new();
static LOGGING_FD: Mutex<Option<std::fs::File>> = Mutex::new(None);
static VPN_SERVICE_CFG: Mutex<Option<crate::easytier::EasyTierTunRequest>> = Mutex::new(None);

// FIXME: Third-party crate 'jni-sys' leaves a dynamic link to JNI_GetCreatedJavaVMs which doesn't exist on Android.
//        A dummy JNI_GetCreatedJavaVMs is declared as a workaround to prevent fatal errors while linking.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn JNI_GetCreatedJavaVMs(_: *mut c_void, _: jsize, _: *mut c_void) -> jint {
    unreachable!();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut c_void) -> jint {
    let registration = || -> jni::errors::Result<jint> {
        let mut jenv = vm.get_env()?;

        let target_class = {
            let clazz = jenv.find_class("java/lang/System")?;
            let method = jenv.get_static_method_id(&clazz, "getProperty", "(Ljava/lang/String;)Ljava/lang/String;")?;

            let key = jenv.new_string("net.burningtnt.terracotta.native_location")?;
            let location = unsafe {
                jenv.call_static_method_unchecked(
                    clazz, method, ReturnType::Object, &[ jvalue { l: key.as_raw() }],
                )?.l()?
            };

            jenv.find_class(
                parse_jstring(&jenv, &JString::from(location)).unwrap_or("net/burningtnt/terracotta/TerracottaAndroidAPI".into())
            )?
        };

        macro_rules! of {
            [$name:expr, $signature:expr, $method:expr] => {
                NativeMethod { name: $name.into(), sig: $signature.into(), fn_ptr: $method as *mut c_void }
            };
        }

        jenv.register_native_methods(target_class, &[
            of!["start0", "(Ljava/lang/String;I)I", jni_start],
            of!["getState0", "()Ljava/lang/String;", jni_get_state],
            of!["setWaiting0", "()V", jni_set_waiting],
            of!["setScanning0", "(Ljava/lang/String;Ljava/lang/String;)V", jni_set_scanning],
            of!["setGuesting0", "(Ljava/lang/String;Ljava/lang/String;)Z", jni_set_guesting],
            of!["verifyRoomCode0", "(Ljava/lang/String;)I", jni_verify_room_code],
            of!["getMetadata0", "()Ljava/lang/String;", jni_get_metadata],
            of!["prepareExportLogs0", "()J", jni_prepare_export_logs],
            of!["finishExportLogs0", "(J)V", jni_finish_export_logs],
            of!["panic0", "()V", jni_panic],
        ])?;

        Ok(JNI_VERSION_1_6)
    };

    registration().unwrap_or_else(|e| {
        let line = format!("Cannot initialize Terracotta Android: {:?}", e);
        logging_android(line.clone());
        panic!("{}", line);
    })
}

extern "system" fn jni_start<'l>(jenv: JNIEnv<'l>, clazz: JClass<'l>, dir: JString<'l>, logging_fd: jint) -> jint {
    std::panic::set_backtrace_style(std::panic::BacktraceStyle::Full);
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

    let _ = LOGGING_FD.lock().unwrap().replace(unsafe { std::fs::File::from_raw_fd(logging_fd) });

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

    let jvm = jenv.get_java_vm().unwrap();
    let clazz = jenv.new_global_ref(clazz).unwrap();

    let dir: String = parse_jstring(&jenv, &dir).unwrap();
    MACHINE_ID_FILE.set(PathBuf::from(dir).join("machine-id"));

    thread::spawn(move || {
        let mut jenv = jvm.attach_current_thread_as_daemon().unwrap();

        let on_vpn_service_sc = jenv.get_static_method_id(
            &clazz, "onVpnServiceStateChanged", "(BBBBSLjava/lang/String;)I",
        ).unwrap();

        loop {
            thread::sleep(Duration::from_millis(1000));

            let Some(cfg) = ({
                VPN_SERVICE_CFG.lock().unwrap().take()
            }) else {
                continue;
            };

            logging!("Android", "Requesting VpnService: ip={}, cidrs: {:?}", cfg.address, cfg.cidrs);
            let [ip1, ip2, ip3, ip4] = cfg.address.octets().map(|i| i as i8);
            let cidrs = cfg.cidrs.join("\0");
            let cidrs2 = jenv.new_string(cidrs).unwrap();

            let tun_fd = unsafe {
                let arguments = [
                    jvalue { b: ip1 }, jvalue { b: ip2 }, jvalue { b: ip3 }, jvalue { b: ip4 },
                    jvalue { s: cfg.network_length as jshort },
                    jvalue { l: cidrs2.into_raw() }
                ];
                jenv.call_static_method_unchecked(&clazz, on_vpn_service_sc, ReturnType::Primitive(Primitive::Int), &arguments)
            };

            match tun_fd {
                Ok(tun_fd) => {
                    let tun_fd = tun_fd.i().unwrap();
                    logging!("Android", "VpnService initialized: tun_fd={}", tun_fd);
                    cfg.dest.write().unwrap().replace(tun_fd);
                }
                Err(jni::errors::Error::JavaException) => {
                    logging!("Android", "Cannot request VpnService: An JavaException is thrown on Java Level.");
                }
                Err(e) => Err(e).unwrap(),
            }
        }
    });

    thread::spawn(|| {
        lazy_static::initialize(&controller::SCAFFOLDING_PORT);
    });

    return 0;
}

fn logging_android(line: String) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    if let Ok(mut fd) = LOGGING_FD.lock() && let Some(fd) = fd.as_mut() {
        let _ = fd.write_all(line.as_bytes());
        let _ = fd.write_all(b"\n");
    }

    let line = CString::new(line).unwrap();
    // SAFETY: 4 is ANDROID_LOG_INFO, while pointers to tag and line are valid.
    unsafe {
        __android_log_write(4, c"hello".as_ptr(), line.as_ptr());
    }
}

extern "system" fn jni_get_state<'l>(jenv: JNIEnv<'l>, _: JClass<'l>) -> JString<'l> {
    try_jvm! { |jenv|
        jenv.new_string(serde_json::to_string(&controller::get_state()).unwrap()).unwrap()
    }
}

extern "system" fn jni_set_waiting<'l>(jenv: JNIEnv<'l>, _: JClass<'l>) {
    try_jvm! { |jenv|
        controller::set_waiting()
    }
}

extern "system" fn jni_set_scanning<'l>(jenv: JNIEnv<'l>, _: JClass<'l>, room: JString<'l>, player: JString<'l>) {
    try_jvm! { |jenv|
        let room = parse_jstring(&jenv, &room);
        let player = parse_jstring(&jenv, &player);
        controller::set_scanning(room, player);
    }
}

extern "system" fn jni_set_guesting<'l>(jenv: JNIEnv<'l>, _: JClass<'l>, room: JString<'l>, player: JString<'l>) -> jboolean {
    try_jvm! { |jenv|
        let room = parse_jstring(&jenv, &room).expect("'room' must not be NULL.");
        let player = parse_jstring(&jenv, &player);

        if let Some(room) = Room::from(&room) && controller::set_guesting(room, player) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }
}

extern "system" fn jni_verify_room_code<'l>(jenv: JNIEnv<'l>, _: JClass<'l>, room: JString<'l>) -> jint {
    try_jvm! { |jenv|
        let room = parse_jstring(&jenv, &room).expect("'room' must not be NULL.");

        match Room::from(&room) {
            Some(Room { kind, .. }) => match kind {
                RoomKind::TerracottaLegacy { .. } => 1,
                RoomKind::PCL2CE { .. } => 2,
                RoomKind::Experimental { .. } => 3
            },
            None => -1
        }
    }
}

extern "system" fn jni_get_metadata<'l>(jenv: JNIEnv<'l>, _: JClass<'l>) -> JString<'l> {
    try_jvm! { |jenv|
        jenv.new_string(format!(
            "{}\0{}\0{}", env!("TERRACOTTA_VERSION"), timestamp::compile_time!() as i64, env!("TERRACOTTA_ET_VERSION")
        )).unwrap()
    }
}

extern "system" fn jni_prepare_export_logs<'l>(jenv: JNIEnv<'l>, _: JClass<'l>) -> jlong {
    try_jvm! { |jenv|
        let mut logging = LOGGING_FD.lock().unwrap();

        if let Some(file) = logging.as_mut() {
            file.flush().unwrap();

            Box::into_raw(Box::new(logging)) as jlong
        } else {
            0
        }
    }
}

extern "system" fn jni_finish_export_logs<'l>(jenv: JNIEnv<'l>, _: JClass<'l>, ptr: jlong) {
    try_jvm! { |jenv|
        unsafe {
            let _ = Box::from_raw(ptr as *mut MutexGuard<Option<File>>);
        }
    }
}

extern "system" fn jni_panic<'l>(jenv: JNIEnv<'l>, _: JClass<'l>) {
    try_jvm! { |jenv|
        panic!("User triggered panic manually.");
    }
}

pub(crate) fn on_vpnservice_change(request: crate::easytier::EasyTierTunRequest) {
    let mut guard = VPN_SERVICE_CFG.lock().unwrap();
    *guard = Some(request);
}

fn parse_jstring<'l>(env: &JNIEnv<'l>, value: &JString<'l>) -> Option<String> {
    if value.is_null() {
        None
    } else {
        // SAFETY: value is a Java String Object
        unsafe { Some(env.get_string_unchecked(value).unwrap().into()) }
    }
}