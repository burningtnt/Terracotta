use std::{
    env, fs,
    io::{self, Read},
    path::Path,
    process, vec,
};

extern crate winres;

fn main() {
    download_easytier();

    sevenz_rust2::compress_to_path(
        "web",
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("webstatics.7z"),
    )
    .unwrap();
    println!("cargo::rerun-if-changed=web");

    if cfg!(target_os = "windows") {
        winres::WindowsResource::new().set_icon_with_id(
            Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("icon.ico")
                .to_str()
                .unwrap(),
            "icon",
        ).compile().unwrap();

        match std::env::var("CARGO_CFG_TARGET_ENV").unwrap().as_str() {
            "gnu" => println!(
                "cargo::rustc-link-arg={}",
                Path::new(&env::var("OUT_DIR").unwrap())
                    .join("resource.o")
                    .to_str()
                    .unwrap()
            ),
            "msvc" => println!(
                "cargo::rustc-link-arg={}",
                Path::new(&env::var("OUT_DIR").unwrap())
                    .join("resource.res")
                    .to_str()
                    .unwrap()
            ),
            _ => panic!(),
        }
        println!("cargo::rerun-if-changed=icon.ico");
    }
}

fn download_easytier() {
    struct EasytierFiles {
        url: &'static str,
        files: Vec<&'static str>,
        entry: &'static str,
        desc: &'static str,
    }

    let conf: EasytierFiles = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-windows-x86_64-v2.3.2.zip",
                files: vec![
                    "easytier-windows-x86_64/easytier-core.exe",
                    "easytier-windows-x86_64/Packet.dll",
                    "easytier-windows-x86_64/wintun.dll",
                ],
                entry: "easytier-core.exe",
                desc: "windows-x86_64"
            }
        } else if cfg!(target_arch = "aarch64") {
            EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-windows-arm64-v2.3.2.zip",
                files: vec![
                    "easytier-windows-arm64/easytier-core.exe",
                    "easytier-windows-arm64/Packet.dll",
                    "easytier-windows-arm64/wintun.dll",
                ],
                entry: "easytier-core.exe",
                desc: "windows-arm64"
            }
        } else {
            panic!("Unsupported target_arch: {}", std::env::consts::ARCH);
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-linux-x86_64-v2.3.2.zip",
                files: vec!["easytier-linux-x86_64/easytier-core"],
                entry: "easytier-core",
                desc: "linux-x86_64"
            }
        } else if cfg!(target_arch = "aarch64") {
            EasytierFiles {
                url: "https://github.com/EasyTier/EasyTier/releases/download/v2.3.2/easytier-linux-aarch64-v2.3.2.zip",
                files: vec!["easytier-linux-aarch64/easytier-core"],
                entry: "easytier-core",
                desc: "linux-arm64"
            }
        }  else {
            panic!("Unsupported target_arch: {}", std::env::consts::ARCH);
        }
    } else {
        panic!("Unsupported target_os: {}", std::env::consts::OS);
    };

    let base = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap()).join(".easytier").join(conf.desc);
    let entry_conf = base.clone().join("entry-conf.v1.txt");
    let entry_archive = base.clone().join("easytier.7z");
    println!("cargo::rustc-env=TERRACOTTA_ET_ENTRY_CONF={}", entry_conf.as_path().to_str().unwrap().to_string());
    println!("cargo::rustc-env=TERRACOTTA_ET_ARCHIVE={}", entry_archive.as_path().to_str().unwrap().to_string());

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
    let target= base.clone().join("easytier.7z.tmp");
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
                        .unwrap().to_str().unwrap().to_string(),
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
