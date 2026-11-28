//! (ours, v26.11.6) "KLBF Lab.exe": double-click it and the settings window opens, with no console
//! window flashing first. It starts this folder's play.ps1 -Setup with PowerShell hidden
//! (CREATE_NO_WINDOW), as "KLBF Lab.cmd" does, which stays beside it as the fallback.
#![windows_subsystem = "windows"]

use std::path::Path;
use std::process::Command;

/// The PowerShell arguments that start the launcher of `dir`.
fn args(dir: &Path) -> Vec<String> {
    let s = |p: &str| dir.join(p).display().to_string();
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"]
        .map(String::from)
        .into_iter()
        .chain([s("play.ps1"), "-Victory".into(), s("victory.exe"), "-Setup".into()])
        .collect()
}

fn main() {
    let exe = std::env::current_exe().expect("own path");
    let dir = exe.parent().expect("own folder");
    let mut cmd = Command::new("powershell.exe");
    cmd.args(args(dir)).current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // ponytail: a failed start shows nothing (no console); play.ps1 reports its own errors in a window.
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn starts_this_folders_launcher_hidden() {
        let a = super::args(std::path::Path::new("C:/KLBF Lab"));
        assert!(a.contains(&"Hidden".to_string()));
        assert!(a.iter().any(|x| x.ends_with("play.ps1")));
        assert!(a.iter().any(|x| x.ends_with("victory.exe")));
        assert_eq!(a.last().unwrap(), "-Setup");
    }
}
