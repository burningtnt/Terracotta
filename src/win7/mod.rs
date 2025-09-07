mod kex;

use crate::win7::kex::KexWinVerSpoof::WinVerSpoofNone;
use std::mem::zeroed;
use std::process::exit;
use winapi::shared::minwindef::{DWORD, WORD};
use winapi::um::winbase::VerifyVersionInfoW;
use winapi::um::winnt::{VerSetConditionMask, DWORDLONG, OSVERSIONINFOEXW, PVOID, VER_GREATER_EQUAL, VER_MAJORVERSION, VER_MINORVERSION, VER_SERVICEPACKMAJOR};
use wmi::{COMLibrary, WMIConnection};

lazy_static::lazy_static! {
    pub static ref WIN7: bool = check_win7();
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tls_callback(_: PVOID, _: u32, _: PVOID) {
    lazy_static::initialize(&WIN7);
}

#[used]
#[unsafe(link_section = ".CRT$XLB")]
static TLS_CALLBACK: unsafe extern "system" fn(PVOID, u32, PVOID) = tls_callback;

fn check_win7() -> bool {
    if let Some(kex) = kex::kex_data_initialize() {
        fail(kex.ifeo_parameters.win_ver_spoof == WinVerSpoofNone, "KEX_WIN_VER_SPOOF: 应不启用 Windows 版本欺骗");
        fail(kex.ifeo_parameters.disable_for_child == 0, "KEX_DISABLE_FOR_CHILD: 应不对子进程启用 VxKex 支持");
    } else {
        fail(is_windows_satisfying(6, 2, 0), "KEX_NOT_AVAILABLE: 请启用 VxKex 支持");
        return false;
    }

    // win7, kex enabled && disable_for_child
    fail(is_windows_satisfying(6, 1, 1), "SYS_WIN7_SP1_NOT_AVAILABLE");
    match COMLibrary::new().and_then(|com| {
        let conn = WMIConnection::new(com)?;
        let fixes = conn.raw_query::<String>("SELECT HotFixID FROM Win32_OperatingSystem")?;

        for fix in ["KB3063858", "KB4474419"] {
            if !fixes.iter().any(|s| s.contains(fix)) {
                return Ok(Some(fix));
            }
        }
        Ok(None)
    }) {
        Ok(Some(fix)) => fail(false, format!("SYS_PATCH_{}", fix)),
        Ok(None) => {}
        Err(e) => fail(false, format!("SYS_WMI_ERR: {}", e)),
    }
    true
}

fn is_windows_satisfying(major: u8, minor: u8, sp: u8) -> bool {
    unsafe {
        let mut osvi = OSVERSIONINFOEXW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as DWORD,
            dwMajorVersion: major as DWORD,
            dwMinorVersion: minor as DWORD,
            wServicePackMajor: sp as WORD,
            ..zeroed()
        };

        let mut condition_mask: DWORDLONG = 0;
        condition_mask = VerSetConditionMask(condition_mask, VER_MAJORVERSION, VER_GREATER_EQUAL);
        condition_mask = VerSetConditionMask(condition_mask, VER_MINORVERSION, VER_GREATER_EQUAL);
        condition_mask = VerSetConditionMask(condition_mask, VER_SERVICEPACKMAJOR, VER_GREATER_EQUAL);

        let res = VerifyVersionInfoW(
            &mut osvi,
            VER_MAJORVERSION | VER_MINORVERSION | VER_SERVICEPACKMAJOR,
            condition_mask,
        );
        res != 0
    }
}

fn fail<T: AsRef<str>>(ok: bool, error: T) {
    if ok {
        return;
    }

    let _ = native_dialog::DialogBuilder::message()
        .set_level(native_dialog::MessageLevel::Error)
        .set_title("Terracotta | 陶瓦联机")
        .set_text(format!("陶瓦联机不支持您的系统，请与开发者联系。\n{}", error.as_ref()))
        .alert()
        .show();
    exit(1);
}