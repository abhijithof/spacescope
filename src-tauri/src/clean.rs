//! Deleting things: tolerant recursive delete that skips locked files, plus the special
//! Windows clean-up actions (empty Recycle Bin, DISM, hibernation...).

use crate::sys;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

#[derive(Default)]
pub struct Stats {
    pub files: u64,
    pub failed: u64,
    pub first_error: Option<String>,
}

fn remove_file_force(p: &Path, st: &mut Stats) {
    match fs::remove_file(p) {
        Ok(_) => st.files += 1,
        Err(e) => {
            // Read-only files can't be deleted until the attribute is cleared.
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                if let Ok(m) = fs::symlink_metadata(p) {
                    let mut perm = m.permissions();
                    if perm.readonly() {
                        #[allow(clippy::permissions_set_readonly_false)]
                        perm.set_readonly(false);
                        if fs::set_permissions(p, perm).is_ok() && fs::remove_file(p).is_ok() {
                            st.files += 1;
                            return;
                        }
                    }
                }
            }
            st.failed += 1;
            if st.first_error.is_none() {
                st.first_error = Some(format!("{}: {}", p.display(), e));
            }
        }
    }
}

/// Deletes everything under `dir` (and `dir` itself unless `keep_root`), skipping anything
/// that's locked or protected. Junctions/symlinks are unlinked, never followed.
pub fn wipe(dir: &Path, keep_root: bool, st: &mut Stats, cancel: &AtomicBool, on_file: &mut dyn FnMut(&Path)) {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            st.failed += 1;
            st.first_error.get_or_insert_with(|| format!("{}: {}", dir.display(), e));
            return;
        }
    };
    for e in rd.flatten() {
        if cancel.load(Relaxed) {
            return;
        }
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            if fs::remove_dir(&p).or_else(|_| fs::remove_file(&p)).is_err() {
                st.failed += 1;
            }
            continue;
        }
        if ft.is_dir() {
            wipe(&p, false, st, cancel, on_file);
        } else {
            remove_file_force(&p, st);
            if st.files % 64 == 0 {
                on_file(&p);
            }
        }
    }
    if !keep_root && fs::remove_dir(dir).is_err() {
        st.failed += 1;
    }
}

pub fn delete_path(path: &str, mode: &str, cancel: &AtomicBool, on_file: &mut dyn FnMut(&Path)) -> Stats {
    let p = Path::new(path);
    let mut st = Stats::default();
    match mode {
        "recycle" => {
            if let Err(e) = sys::recycle(path) {
                st.failed = 1;
                st.first_error = Some(e);
            }
        }
        "contents" => wipe(p, true, &mut st, cancel, on_file),
        _ => {
            let meta = fs::symlink_metadata(p);
            match meta {
                Ok(m) if m.is_dir() => wipe(p, false, &mut st, cancel, on_file),
                Ok(_) => remove_file_force(p, &mut st),
                Err(e) => {
                    st.failed = 1;
                    st.first_error = Some(e.to_string());
                }
            }
        }
    }
    st
}

/// Runs one of the named Windows clean-up actions, scoped to `drive` where that makes
/// sense (the Recycle Bin). Returns a human-readable outcome.
pub fn run_action(action: &str, drive: Option<&str>) -> Result<String, String> {
    let elevated = sys::is_elevated();
    let admin = |exe: &str, args: &str| -> Result<(), String> {
        if elevated {
            let a: Vec<&str> = args.split(' ').collect();
            sys::run_hidden(exe, &a).map(|_| ())
        } else if sys::run_as_admin(exe, args, true) {
            Ok(())
        } else {
            Err("Administrator permission was declined.".into())
        }
    };
    match action {
        "empty-recycle-bin" => {
            let (size, _) = sys::recycle_bin_size(drive);
            sys::empty_recycle_bin(drive)?;
            match drive {
                Some(d) => Ok(format!("Recycle Bin on {d} emptied ({}).", crate::fmt_bytes(size))),
                None => Ok(format!("Recycle Bin emptied ({}).", crate::fmt_bytes(size))),
            }
        }
        "dism-cleanup" => {
            admin("dism.exe", "/Online /Cleanup-Image /StartComponentCleanup /NoRestart")?;
            Ok("Windows Update clean-up finished.".into())
        }
        "hibernate-off" => {
            admin("powercfg.exe", "/hibernate off")?;
            Ok("Hibernation turned off — hiberfil.sys has been removed. Turn it back on any time with `powercfg /hibernate on`.".into())
        }
        "cleanmgr" => {
            if elevated {
                let _ = std::process::Command::new("cleanmgr.exe").spawn();
            } else {
                sys::run_as_admin("cleanmgr.exe", "", false);
            }
            Ok("Disk Cleanup opened. Tick 'Previous Windows installation(s)' / 'Temporary Windows installation files' and press OK.".into())
        }
        "restore-points" => {
            let _ = std::process::Command::new("SystemPropertiesProtection.exe").spawn();
            Ok("System Protection opened. Select the drive → Configure to resize or delete restore points.".into())
        }
        "pagefile" => {
            let _ = std::process::Command::new("SystemPropertiesPerformance.exe").spawn();
            Ok("Performance Options opened. Go to Advanced → Virtual memory → Change.".into())
        }
        "apps" => {
            sys::open_uri("ms-settings:appsfeatures");
            Ok("Settings → Installed apps opened.".into())
        }
        "storage-sense" => {
            sys::open_uri("ms-settings:storagesense");
            Ok("Storage Sense settings opened.".into())
        }
        "steam" => {
            sys::open_uri("steam://open/games");
            Ok("Steam library opened. Right-click a game → Manage → Uninstall.".into())
        }
        _ => Err(format!("Unknown action '{action}'")),
    }
}
