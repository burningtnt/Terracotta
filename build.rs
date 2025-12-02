use sevenz_rust2::encoder_options::{EncoderOptions, LZMA2Options};
use sevenz_rust2::{ArchiveEntry, EncoderConfiguration, EncoderMethod, SourceReader};
use std::io::Cursor;
use std::{
    env, fs,
    io::{self, Read},
    path::Path,
    process, vec,
};

fn main() {
    println!("cargo::rerun-if-changed=Cargo.toml");

    download_easytier();

    sevenz_rust2::compress_to_path(
        "web",
        Path::new(&get_var("OUT_DIR").unwrap()).join("webstatics.7z"),
    )
    .unwrap();
    println!("cargo::rerun-if-changed=web");

    let desc = get_var("TARGET").unwrap().replace('-', "_").to_uppercase();

    let version = get_var("TERRACOTTA_VERSION").unwrap_or_else(|_| "snapshot".to_string());
    println!("cargo::rustc-env=TERRACOTTA_VERSION={}", version);

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

        for vs in ["FileVersion", "ProductVersion"] {
            compiler.set(vs, &version);
        }
        compiler.set_icon("build/windows/icon.ico");
        compiler.compile().unwrap();
    }

    for (key, value) in env::vars() {
        if key.starts_with("CARGO_CFG_") {
            println!("cargo::rustc-env={}={}", key, value);
        }
    }
}

fn download_easytier() {
    struct EasytierFiles {
        url: &'static str,
        files: Vec<&'static str>,
        entry: &'static str,
        cli: &'static str,
        desc: &'static str,
    }

    let version = {
        let mut input = fs::read_to_string(
            Path::new(&get_var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml"),
        )
            .unwrap()
            .parse::<toml::Table>()
            .unwrap();

        for key in "package.metadata.easytier".split(".") {
            input = match input.into_iter().find(|(k, _)| k == key).unwrap().1 {
                toml::Value::Table(map) => map,
                _ => panic!("Expecting a table for key: {}", key),
            }
        }

        input.get("version").unwrap().as_str().unwrap().to_string()
    };
    println!("cargo::rustc-env=TERRACOTTA_ET_VERSION={}", version);

    let target_os = get_var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = get_var("CARGO_CFG_TARGET_ARCH").unwrap();

    let conf = match (target_os.as_str(), target_arch.as_str()) {
        ("windows", "x86_64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-windows-x86_64-{V}.zip",
            files: vec![
                "easytier-windows-x86_64/easytier-core.exe",
                "easytier-windows-x86_64/easytier-cli.exe",
                "easytier-windows-x86_64/Packet.dll",
            ],
            entry: "easytier-core.exe",
            cli: "easytier-cli.exe",
            desc: "windows-x86_64",
        },
        ("windows", "aarch64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-windows-arm64-{V}.zip",
            files: vec![
                "easytier-windows-arm64/easytier-core.exe",
                "easytier-windows-arm64/easytier-cli.exe",
                "easytier-windows-arm64/Packet.dll",
            ],
            entry: "easytier-core.exe",
            cli: "easytier-cli.exe",
            desc: "windows-arm64",
        },
        ("linux", "x86_64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-linux-x86_64-{V}.zip",
            files: vec![
                "easytier-linux-x86_64/easytier-core",
                "easytier-linux-x86_64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "linux-x86_64",
        },
        ("linux", "aarch64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-linux-aarch64-{V}.zip",
            files: vec![
                "easytier-linux-aarch64/easytier-core",
                "easytier-linux-aarch64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "linux-arm64",
        },
        ("linux", "riscv64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-linux-riscv64-{V}.zip",
            files: vec![
                "easytier-linux-riscv64/easytier-core",
                "easytier-linux-riscv64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "linux-riscv64",
        },
        ("linux", "loongarch64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-linux-loongarch64-{V}.zip",
            files: vec![
                "easytier-linux-loongarch64/easytier-core",
                "easytier-linux-loongarch64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "linux-loongarch64",
        },
        ("macos", "x86_64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-macos-x86_64-{V}.zip",
            files: vec![
                "easytier-macos-x86_64/easytier-core",
                "easytier-macos-x86_64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "macos-x86_64",
        },
        ("macos", "aarch64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-macos-aarch64-{V}.zip",
            files: vec![
                "easytier-macos-aarch64/easytier-core",
                "easytier-macos-aarch64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "macos-arm64",
        },
        ("freebsd", "x86_64") => EasytierFiles {
            url: "https://github.com/burningtnt/EasyTier/releases/download/{V}/easytier-freebsd-13.2-x86_64-{V}.zip",
            files: vec![
                "easytier-freebsd-13.2-x86_64/easytier-core",
                "easytier-freebsd-13.2-x86_64/easytier-cli",
            ],
            entry: "easytier-core",
            cli: "easytier-cli",
            desc: "freebsd-x86_64",
        },
        ("android", "arm") | ("android", "aarch64") | ("android", "x86") | ("android", "x86_64") => return,
        _ => panic!(
            "Cannot compile Terracotta on {}-{}: Cannot find valid EasyTier binary.",
            target_os, target_arch
        ),
    };

    println!("cargo::rerun-if-changed=.easytier");

    let base = Path::new(&get_var("CARGO_MANIFEST_DIR").unwrap())
        .join(".easytier")
        .join(&version)
        .join(conf.desc);
    let entry_conf = base.clone().join("entry-conf.v1.txt");
    let cli_conf = base.clone().join("cli-conf.v1.txt");
    let entry_archive = base.clone().join("easytier.7z");
    println!(
        "cargo::rustc-env=TERRACOTTA_ET_ENTRY_CONF={}",
        entry_conf.as_path().to_str().unwrap()
    );
    println!(
        "cargo::rustc-env=TERRACOTTA_ET_CLI_CONF={}",
        cli_conf.as_path().to_str().unwrap()
    );
    println!(
        "cargo::rustc-env=TERRACOTTA_ET_ARCHIVE={}",
        entry_archive.as_path().to_str().unwrap()
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

    reqwest::blocking::get(conf.url.replace("{V}", &version))
        .unwrap()
        .copy_to(&mut io::BufWriter::new(
            fs::File::create(source.clone()).unwrap(),
        ))
        .inspect_err(|_| {
            let _ = fs::remove_file(source.clone());
        })
        .unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(source.clone()).unwrap()).unwrap();
    let target = base.clone().join("easytier.7z.tmp");
    let mut writer =
        sevenz_rust2::ArchiveWriter::new(fs::File::create(target.clone()).unwrap()).unwrap();
    writer.set_content_methods(vec![EncoderConfiguration {
        method: EncoderMethod::LZMA2,
        options: Some(EncoderOptions::LZMA2(LZMA2Options::from_level(9))),
    }, EncoderConfiguration {
        method: match target_arch.as_str() {
            "x86_64" => EncoderMethod::BCJ_X86_FILTER,
            "aarch64" => EncoderMethod::BCJ_ARM64_FILTER,
            "riscv64" => EncoderMethod::BCJ_RISCV_FILTER,
            _ => EncoderMethod::COPY
        },
        options: None,
    }]);
    let mut archive_entries: Vec<ArchiveEntry> = vec![];
    let mut archive_readers: Vec<SourceReader<Cursor<Vec<u8>>>> = vec![];
    for file in conf.files.iter() {
        let mut entry = archive.by_name(file).unwrap();

        let mut buf: Vec<u8> = vec![];
        entry.read_to_end(&mut buf).unwrap();

        archive_entries.push(ArchiveEntry::new_file(Path::new(&entry.enclosed_name().unwrap()).file_name().unwrap().to_str().unwrap()));
        archive_readers.push(SourceReader::new(Cursor::new(buf)));
    }
    writer.push_archive_entries(archive_entries, archive_readers).unwrap();
    writer.finish().unwrap();

    let r = fs::rename(target.clone(), entry_archive.clone());
    if fs::metadata(entry_archive.clone()).is_err() {
        r.unwrap();
    }
    fs::write(entry_conf, conf.entry).unwrap();
    fs::write(cli_conf, conf.cli).unwrap();
}

pub fn get_var<K: AsRef<std::ffi::os_str::OsStr>>(key: K) -> Result<String, env::VarError> {
    println!(
        "cargo::rerun-if-env-changed={}",
        key.as_ref().to_string_lossy()
    );
    env::var(key.as_ref())
}
