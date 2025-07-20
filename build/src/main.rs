use std::{
    env, fs,
    io::{self, Write},
};

fn main() {
    enum TargetTransform {
        NONE,
        TAR,
    }

    struct Target {
        toolchain: &'static str,
        executable: &'static str,
        classifier: &'static str,
        transform: TargetTransform,
    }

    impl Target {
        pub fn open(&self) -> fs::File {
            return fs::OpenOptions::new()
                .read(true)
                .open(env::current_dir().unwrap().join(format!(
                    "target/{}/release/{}",
                    self.toolchain, self.executable
                )))
                .unwrap();
        }
    }

    let targets: Vec<Target> = vec![
        Target {
            toolchain: "x86_64-pc-windows-gnullvm",
            executable: "terracotta.exe",
            classifier: "windows-x86_64.exe",
            transform: TargetTransform::NONE,
        },
        Target {
            toolchain: "aarch64-pc-windows-gnullvm",
            executable: "terracotta.exe",
            classifier: "windows-aarch64.exe",
            transform: TargetTransform::NONE,
        },
        Target {
            toolchain: "x86_64-unknown-linux-gnu",
            executable: "terracotta",
            classifier: "linux-x86_64-gnu",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "aarch64-unknown-linux-gnu",
            executable: "terracotta",
            classifier: "linux-aarch64-gnu",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "x86_64-unknown-linux-musl",
            executable: "terracotta",
            classifier: "linux-x86_64-musl",
            transform: TargetTransform::TAR,
        },
        Target {
            toolchain: "aarch64-unknown-linux-musl",
            executable: "terracotta",
            classifier: "linux-aarch64-musl",
            transform: TargetTransform::TAR,
        },
    ];

    let mut writer = zip::ZipWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(env::var("TERRACOTTA_ARTIFACT").unwrap())
            .unwrap(),
    );

    for target in targets.iter() {
        let name = format!(
            "terracotta-{}-{}",
            env!("CARGO_PKG_VERSION"),
            target.classifier
        );
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Zstd)
            .compression_level(Some(22))
            .unix_permissions(0o777);

        match target.transform {
            TargetTransform::NONE => {
                writer.start_file(&name, options).unwrap();
                io::copy(&mut target.open(), &mut writer).unwrap();
            }
            TargetTransform::TAR => {
                let mut buffer = vec![];

                let mut header = tar::Header::new_gnu();
                header.set_size(target.open().metadata().unwrap().len());
                header.set_cksum();
                tar::Builder::new(&mut buffer)
                    .append_data(&mut header, &name, &mut target.open())
                    .unwrap();

                writer
                    .start_file(format!("{}.tar", &name), options)
                    .unwrap();
                writer.write_all(&mut buffer).unwrap();
            }
        }
    }

    writer.finish().unwrap();
}
