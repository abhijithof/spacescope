//! Thin wrappers around Windows APIs: drives, elevation, recycle bin, processes, installed apps.

use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevation, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHFileOperationW, SHQueryRecycleBinW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FO_DELETE, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHFILEOPSTRUCTW, SHQUERYRBINFO,
};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn long_path(p: &str) -> String {
    if p.starts_with(r"\\") {
        p.to_string()
    } else {
        format!(r"\\?\{p}")
    }
}

#[derive(Default)]
pub struct VolInfo {
    pub total: u64,
    pub free: u64,
    pub fs: String,
    pub label: String,
}

pub fn volume_info(root: &str) -> VolInfo {
    let w = wide(root);
    let mut v = VolInfo::default();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    unsafe {
        if GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) != 0 {
            v.total = total;
            v.free = free;
        }
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        if GetVolumeInformationW(
            w.as_ptr(),
            label.as_mut_ptr(),
            261,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs.as_mut_ptr(),
            261,
        ) != 0
        {
            v.label = from_wide(&label);
            v.fs = from_wide(&fs);
        }
    }
    v
}

fn from_wide(b: &[u16]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..n])
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub letter: String,
    pub label: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub free: u64,
}

/// Local drives (fixed + removable). Network shares and optical drives are left out.
pub fn drives() -> Vec<DriveInfo> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i as u8) as char);
        let root = format!("{letter}\\");
        let t = unsafe { GetDriveTypeW(wide(&root).as_ptr()) };
        let kind = match t {
            2 => "removable",
            3 => "fixed",
            6 => "ram",
            _ => continue,
        };
        let v = volume_info(&root);
        if v.total == 0 {
            continue;
        }
        out.push(DriveInfo { letter, label: v.label, fs: v.fs, kind: kind.into(), total: v.total, free: v.free });
    }
    out
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut tok: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
            return false;
        }
        let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(tok, TokenElevation, &mut el as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        CloseHandle(tok);
        ok != 0 && el.TokenIsElevated != 0
    }
}

/// With admin rights, SeBackupPrivilege lets the scanner list folders whose ACLs would
/// otherwise deny us (other users' profiles, System Volume Information, WindowsApps...).
pub fn enable_backup_privileges() {
    for name in ["SeBackupPrivilege", "SeRestorePrivilege"] {
        unsafe {
            let mut tok: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok) == 0 {
                continue;
            }
            let mut luid = LUID { LowPart: 0, HighPart: 0 };
            if LookupPrivilegeValueW(std::ptr::null(), wide(name).as_ptr(), &mut luid) != 0 {
                let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
                AdjustTokenPrivileges(tok, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut());
            }
            CloseHandle(tok);
        }
    }
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Starts `exe` elevated through a UAC prompt. Returns false if the user declined.
pub fn run_as_admin(exe: &str, args: &str, wait: bool) -> bool {
    let mut cmd = format!("Start-Process -FilePath {} -Verb RunAs", ps_quote(exe));
    if !args.is_empty() {
        cmd.push_str(&format!(" -ArgumentList {}", ps_quote(args)));
    }
    if wait {
        cmd.push_str(" -Wait");
    }
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn run_hidden(exe: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{} {}", text.trim(), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn open_in_explorer(path: &str, select: bool) {
    let mut c = Command::new("explorer.exe");
    if select {
        c.raw_arg(format!("/select,\"{path}\""));
    } else {
        c.raw_arg(format!("\"{path}\""));
    }
    let _ = c.spawn();
}

pub fn open_uri(uri: &str) {
    let _ = Command::new("cmd").args(["/C", "start", "", uri]).creation_flags(CREATE_NO_WINDOW).spawn();
}

/// Sends a file or folder to the Recycle Bin. Windows will warn (rather than silently
/// delete) if the item is too large to be recycled.
pub fn recycle(path: &str) -> Result<(), String> {
    let mut w: Vec<u16> = path.encode_utf16().collect();
    w.push(0);
    w.push(0);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE as _;
    op.pFrom = w.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING) as _;
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        Err(format!("Windows refused to recycle it (code {r:#x}) — the item may be open in another program."))
    } else if op.fAnyOperationsAborted != 0 {
        Err("Cancelled.".into())
    } else {
        Ok(())
    }
}

/// Size and item count of one drive's Recycle Bin (`Some("C:")`) or all of them (`None`).
pub fn recycle_bin_size(drive: Option<&str>) -> (u64, u64) {
    let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    let root = drive.map(|d| wide(&format!("{d}\\")));
    unsafe { SHQueryRecycleBinW(root.as_ref().map_or(std::ptr::null(), |r| r.as_ptr()), &mut info) };
    (info.i64Size as u64, info.i64NumItems as u64)
}

pub fn empty_recycle_bin(drive: Option<&str>) -> Result<(), String> {
    let root = drive.map(|d| wide(&format!("{d}\\")));
    let r = unsafe {
        SHEmptyRecycleBinW(
            std::ptr::null_mut(),
            root.as_ref().map_or(std::ptr::null(), |r| r.as_ptr()),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    };
    // S_OK, or E_UNEXPECTED when it was already empty.
    if r == 0 || r as u32 == 0x8000_FFFF {
        Ok(())
    } else {
        Err(format!("Emptying the Recycle Bin failed (code {:#x})", r as u32))
    }
}

/// Lower-cased executable names of running processes.
pub fn running_processes() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                out.push(from_wide(&e.szExeFile).to_lowercase());
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out.sort();
    out.dedup();
    out
}

/// Names that identify installed software: display names, publishers and install folders
/// from the Uninstall registry keys, plus Microsoft Store package names.
pub fn installed_app_names() -> Vec<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut out = Vec::new();
    let roots = [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];
    for (hive, path) in roots {
        let Ok(key) = RegKey::predef(hive).open_subkey(path) else { continue };
        for sub in key.enum_keys().flatten() {
            out.push(sub.clone());
            let Ok(k) = key.open_subkey(&sub) else { continue };
            for v in ["DisplayName", "Publisher"] {
                if let Ok(s) = k.get_value::<String, _>(v) {
                    out.push(s);
                }
            }
            for v in ["InstallLocation", "DisplayIcon", "UninstallString"] {
                if let Ok(s) = k.get_value::<String, _>(v) {
                    let s = s.trim_matches('"').split(".exe").next().unwrap_or("").to_string();
                    let parts: Vec<&str> = s.split('\\').filter(|p| !p.is_empty()).collect();
                    // keep the folder names just below Program Files / AppData roots
                    for p in parts.iter().skip(2).take(3) {
                        out.push(p.to_string());
                    }
                }
            }
        }
    }
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        if let Ok(rd) = std::fs::read_dir(format!("{la}\\Packages")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                let fam = n.split('_').next().unwrap_or("").to_string();
                for part in fam.split('.') {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}
