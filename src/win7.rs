use std::mem::zeroed;
use std::process::exit;
use winapi::shared::minwindef::{DWORD, WORD};
use winapi::um::winbase::VerifyVersionInfoW;
use winapi::um::winnt::{VerSetConditionMask, VER_GREATER_EQUAL, VER_MAJORVERSION, VER_MINORVERSION, VER_SERVICEPACKMAJOR};
use winapi::um::winnt::{DWORDLONG, OSVERSIONINFOEXW};
use wmi::{COMLibrary, WMIConnection};

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
            condition_mask
        );
        res != 0
    }
}

lazy_static::lazy_static! {
    pub static ref WIN7: bool = {
        check();
        true
    };
}

fn check() {
    if is_windows_satisfying(6, 2, 0) {
        return;
    }

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
        Ok(None) => {},
        Err(e) => fail(false, format!("SYS_WMI_ERR: {}", e)),
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