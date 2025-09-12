use std::ffi::OsStr;
use std::marker::PhantomData;
use std::mem::{transmute, zeroed};
use std::os::windows::ffi::OsStrExt;
use winapi::shared::minwindef::{FARPROC, HMODULE};
use winapi::shared::ntdef::{UNICODE_STRING, HANDLE, PVOID, ULONG, NTSTATUS};
use winapi::um::libloaderapi::{FreeLibrary, GetProcAddress, LoadLibraryW};
use winapi::um::winnt::LPCSTR;
use crate::win7::fail;

#[allow(dead_code)]
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum KexWinVerSpoof {
    WinVerSpoofNone = 0,
    WinVerSpoofWin7,
    WinVerSpoofWin8,
    WinVerSpoofWin8Point1,
    WinVerSpoofWin10,
    WinVerSpoofWin11,
    WinVerSpoofMax,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct KexIfeoParameters {
    pub disable_for_child: ULONG,
    pub disable_app_specific: ULONG,
    pub win_ver_spoof: KexWinVerSpoof,
    pub strong_version_spoof: ULONG,
}

#[repr(C)]
pub struct VKLContext {
    _phantom: PhantomData<()>,
}

#[repr(C)]
pub struct KexProcessData {
    pub flags: ULONG,
    pub ifeo_parameters: KexIfeoParameters,
    pub win_dir: UNICODE_STRING,
    pub kex_dir: UNICODE_STRING,
    pub log_dir: UNICODE_STRING,
    pub kex3264dir_path: UNICODE_STRING,
    pub image_base_name: UNICODE_STRING,
    pub log_handle: *mut VKLContext,
    pub kex_dll_base: PVOID,
    pub system_dll_base: PVOID,
    pub native_system_dll_base: PVOID,
    pub base_dll_base: PVOID,
    pub base_named_objects: HANDLE,
    pub untrusted_named_objects: HANDLE,
    pub ksec_dd: HANDLE,
}

pub fn kex_data_initialize() -> Option<KexProcessData> {
    unsafe {
        let path: Vec<u16> = OsStr::new("kexdll.dll\0").encode_wide().collect();
        let module: HMODULE = LoadLibraryW(path.as_ptr());
        if module.is_null() {
            return None;
        }
        let initialize: FARPROC = GetProcAddress(module, c"KexDataInitialize".as_ptr() as LPCSTR);
        fail(!initialize.is_null(), "KEX_INCOMPLETE: VxKex 不完整");
        let initialize: unsafe extern "system" fn(*mut KexProcessData) -> NTSTATUS = transmute(initialize);

        let mut data: KexProcessData = zeroed();
        initialize(&mut data);

        FreeLibrary(module);
        Some(data)
    }
}
