//! (ours) The KLBF Lab icon on "KLBF Lab.exe": the same `klbf-lab.rc` as victory.exe, built with the
//! Windows SDK's `rc.exe`; without it the build goes on with no icon and one warning.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../victory/klbf-lab.rc");
    println!("cargo:rerun-if-changed=../victory/klbf-lab.ico");
    if !std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|v| v == "msvc") {
        return;
    }
    let rc = std::env::var_os("RC").map(PathBuf::from).or_else(|| {
        let bin = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
        let mut found: Vec<PathBuf> =
            std::fs::read_dir(bin).ok()?.flatten().map(|e| e.path().join("x64/rc.exe")).filter(|p| p.is_file()).collect();
        found.sort();
        found.pop()
    });
    let Some(rc) = rc else {
        println!("cargo:warning=no rc.exe (Windows SDK): KLBF Lab.exe has no icon");
        return;
    };
    let res = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("klbf-lab.res");
    match Command::new(&rc).current_dir("../victory").arg("/nologo").arg("/fo").arg(&res).arg("klbf-lab.rc").status() {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        other => println!("cargo:warning={} failed ({other:?}): no icon", rc.display()),
    }
}
