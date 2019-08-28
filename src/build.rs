use std::env;
use std::env::consts::ARCH;
use std::fs::File;
use std::io::Error;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use rustc_version::version;

fn save_platform() -> Result<(), Error> {
    let root_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let dest_path = Path::new(&root_dir)
        .join("target")
        .join(&profile)
        .join("target.txt");
    let mut f = File::create(&dest_path)?;
    f.write_all(env::var("TARGET").unwrap().as_bytes())
}

fn load_revision() -> Result<String, Error> {
    let output = Command::new("git")
        .arg("log")
        .arg("-n1")
        .arg("--format=%H")
        .output()?;
    Ok(String::from_utf8(output.stdout).unwrap().trim().to_string())
}

fn save_version() -> Result<(), Error> {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("version.rs");
    let mut f = File::create(&dest_path).unwrap();
    f.write_all(
        &format!(
            r#"
pub const REVISION: &str = "{revision}";
pub const RUSTC: &str = "{rustc}";
"#,
            revision = load_revision()?,
            rustc = version().unwrap(),
        )
        .into_bytes(),
    )
}

fn save_control() -> Result<(), Error> {
    let root_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let dest_path = Path::new(&root_dir)
        .join("target")
        .join(&profile)
        .join("version.sh");
    let arch = match ARCH {
        "x86_64" => "amd64",
        other => other,
    };
    let mut f = File::create(&dest_path).unwrap();
    f.write_all(
        &format!(
            r#"
VERSION={version}
ARCH={arch}
REVISION={revision}
"#,
            arch = arch,
            revision = load_revision()?,
            version = env::var("CARGO_PKG_VERSION").unwrap(),
        )
        .into_bytes(),
    )
}

fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("src/schema")
        .file("src/schema/builder.capnp")
        .run()
        .unwrap();
    save_platform().unwrap();
    save_version().unwrap();
    save_control().unwrap();
}
