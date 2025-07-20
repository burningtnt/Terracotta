use std::{
    env, fs,
    io::{self, Read},
    path::Path,
    process, vec,
};

fn main() {
    download_easytier();

    sevenz_rust2::compress_to_path(
        "web",
        Path::new(&get_var("OUT_DIR").unwrap()).join("webstatics.7z"),
    )
    .unwrap();
    println!("cargo::rerun-if-changed=web");

    let desc = get_var("TARGET").unwrap().replace('-', "_").to_uppercase();

    let target_family = get_var("CARGO_CFG_TARGET_FAMILY").unwrap().to_string();
    if target_family == "windows" {
        println!("cargo::rerun-if-changed=build/windows/icon.ico");
        let mut compiler = winresource::WindowsResource::new();

        {
            let current = Path::new(&get_var("CARGO_MANIFEST_DIR").unwrap()).to_owned();

            if let Ok(windres) = get_var(&format!("CARGO_TARGET_{}_WINDRES_PATH", desc)) {
                let windres = current.join(windres);
                compiler.set_windres_path(windres.to_str().unwrap());
            }
            if let Ok(ar) = get_var(&format!("CARGO_TARGET_{}_AR", desc)) {
                let ar = current.join(ar);
                compiler.set_ar_path(ar.to_str().unwrap());
            }
        }

        compiler.set_icon("build/windows/icon.ico");
        compiler.compile().unwrap();
    }

    // if let Ok(value) = get_var(&format!("CARGO_TARGET_{}_RUSTFLAGS", desc)) {
    //     for ele in value.split(" ") {
    //         println!("cargo::rustc-link-arg={}", ele)
    //     }
    // }
}

fn download_easytier() {
    struct EasytierFiles {
        url: &'static str,
        files: Vec<&'static str>,
        entry: &'static str,
        desc: &'static str,
    }

    let target_os = get_var("CARGO_CFG_TARGET_OS").unwrap().to_string();
    let target_arch = get_var("CARGO_CFG_TARGET_ARCH").unwrap().to_string();
    let conf = match target_os.as_str() {
        "windows" => match target_arch.as_str() {
            "x86_64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-windows-x86_64-v2.3.2.zip",
                files: vec![
                    "easytier-windows-x86_64/easytier-core.exe",
                    "easytier-windows-x86_64/Packet.dll",
                    "easytier-windows-x86_64/wintun.dll",
                ],
                entry: "easytier-core.exe",
                desc: "windows-x86_64",
            },
            "aarch64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-windows-arm64-v2.3.2.zip",
                files: vec![
                    "easytier-windows-arm64/easytier-core.exe",
                    "easytier-windows-arm64/Packet.dll",
                    "easytier-windows-arm64/wintun.dll",
                ],
                entry: "easytier-core.exe",
                desc: "windows-arm64",
            },
            _ => panic!("Unsupported target arch: {}", target_arch),
        },
        "linux" => match target_arch.as_str() {
            "x86_64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-linux-x86_64-v2.3.2.zip",
                files: vec!["easytier-linux-x86_64/easytier-core"],
                entry: "easytier-core",
                desc: "linux-x86_64",
            },
            "aarch64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-linux-aarch64-v2.3.2.zip",
                files: vec!["easytier-linux-aarch64/easytier-core"],
                entry: "easytier-core",
                desc: "linux-arm64",
            },
            _ => panic!("Unsupported target arch: {}", target_arch),
        },
        "macos" => match target_arch.as_str() {
            "x86_64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-macos-x86_64-v2.3.2.zip",
                files: vec!["easytier-macos-x86_64/easytier-core"],
                entry: "easytier-core",
                desc: "macos-x86_64",
            },
            "aarch64" => EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-macos-aarch64-v2.3.2.zip",
                files: vec!["easytier-macos-aarch64/easytier-core"],
                entry: "easytier-core",
                desc: "macos-arm64",
            },
            _ => panic!("Unsupported target arch: {}", target_arch),
        },
        _ => panic!("Unsupported target os: {}", target_os),
    };

    let base = Path::new(&get_var("CARGO_MANIFEST_DIR").unwrap())
        .join(".easytier")
        .join(conf.desc);
    let entry_conf = base.clone().join("entry-conf.v1.txt");
    let entry_archive = base.clone().join("easytier.7z");
    println!(
        "cargo::rustc-env=TERRACOTTA_ET_ENTRY_CONF={}",
        entry_conf.as_path().to_str().unwrap().to_string()
    );
    println!(
        "cargo::rustc-env=TERRACOTTA_ET_ARCHIVE={}",
        entry_archive.as_path().to_str().unwrap().to_string()
    );

    if fs::metadata(entry_conf.clone()).is_ok() {
        return;
    }

    if fs::metadata(base.clone()).is_ok() {
        fs::remove_dir_all(base.clone()).unwrap();
    }
    fs::create_dir_all(base.clone()).unwrap();

    let source =
        Path::new(&env::temp_dir()).join(format!("terracotta-build-rs-{}.zip", process::id()));

    let result = reqwest::blocking::get(conf.url)
        .unwrap()
        .copy_to(&mut io::BufWriter::new(
            fs::File::create(source.clone()).unwrap(),
        ));
    if result.is_err() {
        let _ = fs::remove_file(source.clone());
        result.unwrap();
    }

    let mut archive = zip::ZipArchive::new(fs::File::open(source.clone()).unwrap()).unwrap();
    let target = base.clone().join("easytier.7z.tmp");
    let mut writer =
        sevenz_rust2::ArchiveWriter::new(fs::File::create(target.clone()).unwrap()).unwrap();

    for file in conf.files.iter() {
        let mut buf: std::vec::Vec<u8> = vec![];

        let mut entry = archive.by_name(file).unwrap();
        entry.read_to_end(&mut buf).unwrap();

        writer
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::from_path(
                    "",
                    Path::new(&entry.enclosed_name().unwrap())
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string(),
                ),
                Some(io::Cursor::new(buf)),
            )
            .unwrap();
    }

    writer.finish().unwrap();
    let r = fs::rename(target.clone(), entry_archive.clone());
    if !fs::metadata(entry_archive.clone()).is_ok() {
        r.unwrap();
    }
    fs::write(entry_conf, conf.entry).unwrap();
}

pub fn get_var<K: core::convert::AsRef<std::ffi::os_str::OsStr>>(key: K) -> core::result::Result<String, std::env::VarError> {
    println!("cargo::rerun-if-env-changed={}", key.as_ref().to_string_lossy());
    return env::var(key.as_ref());
}