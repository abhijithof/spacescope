mod ai;
mod analyze;
mod clean;
mod kb;
mod scan;
mod sys;
mod tree;

use analyze::*;
use kb::Kb;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, State};
use tree::{ScanMeta, Tree};

pub struct App {
    trees: RwLock<HashMap<String, Tree>>,
    kb: Kb,
    cancel: Arc<AtomicBool>,
    busy: Mutex<Option<String>>,
    loaded: Mutex<bool>,
}

type St<'a> = State<'a, Arc<App>>;
type R<T> = Result<T, String>;

pub fn fmt_bytes(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", units[i])
    }
}

fn cache_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("SpaceScope").join("scans")
}

fn cache_file(drive: &str) -> PathBuf {
    cache_dir().join(format!("{}.bin", drive.trim_end_matches(':')))
}

fn save_tree(t: &Tree) {
    if let Err(e) = t.save(&cache_file(&t.drive)) {
        eprintln!("saving scan of {} failed: {e}", t.drive);
    }
}

/// Holds the "busy" slot for the duration of a long operation.
struct BusyGuard(Arc<App>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap() = None;
    }
}

fn claim(app: &Arc<App>, what: &str) -> R<BusyGuard> {
    let mut b = app.busy.lock().unwrap();
    if let Some(cur) = &*b {
        return Err(format!("Please wait — {cur} is still running."));
    }
    *b = Some(what.to_string());
    app.cancel.store(false, Relaxed);
    Ok(BusyGuard(app.clone()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DriveRow {
    #[serde(flatten)]
    info: sys::DriveInfo,
    scan: Option<ScanMeta>,
    root_alloc: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitInfo {
    drives: Vec<DriveRow>,
    elevated: bool,
    busy: Option<String>,
}

fn drive_rows(app: &App) -> Vec<DriveRow> {
    let trees = app.trees.read().unwrap();
    sys::drives()
        .into_iter()
        .map(|d| {
            let t = trees.get(&d.letter);
            DriveRow { scan: t.map(|t| t.meta.clone()), root_alloc: t.map(|t| t.node(0).alloc), info: d }
        })
        .collect()
}

/// Loads the previous scans from disk. This never touches the drives themselves —
/// the app only scans when you press Scan.
#[tauri::command]
async fn init(app: St<'_>) -> R<InitInfo> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut loaded = app.loaded.lock().unwrap();
        if !*loaded {
            *loaded = true;
            if let Ok(rd) = std::fs::read_dir(cache_dir()) {
                let files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bin")).collect();
                let loaded_trees: Vec<Tree> = std::thread::scope(|s| {
                    let hs: Vec<_> = files.iter().map(|f| s.spawn(move || Tree::load(f))).collect();
                    hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
                });
                let mut trees = app.trees.write().unwrap();
                for t in loaded_trees {
                    trees.insert(t.drive.clone(), t);
                }
            }
        }
        drop(loaded);
        Ok(InitInfo { drives: drive_rows(&app), elevated: sys::is_elevated(), busy: app.busy.lock().unwrap().clone() })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn drives(app: St<'_>) -> R<Vec<DriveRow>> {
    Ok(drive_rows(&app))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    drive: String,
    index: usize,
    of: usize,
    files: u64,
    dirs: u64,
    bytes: u64,
    denied: u64,
    current: String,
    elapsed_ms: u64,
}

#[tauri::command]
async fn start_scan(app: St<'_>, handle: AppHandle, drives: Vec<String>, gentle: bool) -> R<()> {
    let app = app.inner().clone();
    let guard = claim(&app, "a scan")?;
    std::thread::spawn(move || {
        let _guard = guard;
        let t0 = std::time::Instant::now();
        let mut cancelled = false;
        for (i, d) in drives.iter().enumerate() {
            let prog = Arc::new(scan::Progress::default());
            let done = Arc::new(AtomicBool::new(false));
            let ticker = {
                let (prog, done, h, d) = (prog.clone(), done.clone(), handle.clone(), d.clone());
                let n = drives.len();
                std::thread::spawn(move || {
                    while !done.load(Relaxed) {
                        let p = ScanProgress {
                            drive: d.clone(),
                            index: i,
                            of: n,
                            files: prog.files.load(Relaxed),
                            dirs: prog.dirs.load(Relaxed),
                            bytes: prog.bytes.load(Relaxed),
                            denied: prog.denied.load(Relaxed),
                            current: prog.current.lock().map(|c| c.clone()).unwrap_or_default(),
                            elapsed_ms: t0.elapsed().as_millis() as u64,
                        };
                        let _ = h.emit("scan-progress", p);
                        std::thread::sleep(std::time::Duration::from_millis(150));
                    }
                })
            };
            let result = scan::scan_drive(d, gentle, prog, app.cancel.clone());
            done.store(true, Relaxed);
            let _ = ticker.join();
            match result {
                Some(t) => {
                    let _ = handle.emit("scan-saving", d.clone());
                    save_tree(&t);
                    app.trees.write().unwrap().insert(d.clone(), t);
                }
                None => {
                    cancelled = true;
                    break;
                }
            }
        }
        let _ = handle.emit("scan-done", cancelled);
    });
    Ok(())
}

#[tauri::command]
async fn cancel(app: St<'_>) -> R<()> {
    app.cancel.store(true, Relaxed);
    Ok(())
}

fn with_tree<T>(app: &App, drive: &str, f: impl FnOnce(&Tree) -> T) -> R<T> {
    let trees = app.trees.read().unwrap();
    let t = trees.get(drive).ok_or_else(|| format!("{drive} hasn't been scanned yet."))?;
    Ok(f(t))
}

#[tauri::command]
async fn treemap(app: St<'_>, drive: String, id: u32, depth: u32, by_type: bool) -> R<Tm> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_tree(&app, &drive, |t| {
            let id = if t.valid(id) { id } else { 0 };
            analyze::treemap(t, &app.kb, id, depth.clamp(1, 4), by_type)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn children(app: St<'_>, drive: String, id: u32) -> R<Vec<NodeLite>> {
    with_tree(&app, &drive, |t| if t.valid(id) { analyze::children(t, &app.kb, id, 2000) } else { vec![] })
}

#[tauri::command]
async fn detail(app: St<'_>, drive: String, id: u32) -> R<Detail> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_tree(&app, &drive, |t| if t.valid(id) { Ok(analyze::detail(t, &app.kb, id)) } else { Err("That item no longer exists.".to_string()) })?
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn locate(app: St<'_>, path: String) -> R<Option<(String, u32)>> {
    let trees = app.trees.read().unwrap();
    let drive = path.get(..2).unwrap_or("").to_uppercase();
    Ok(trees.get(&drive).and_then(|t| t.find_path(&path)).map(|id| (drive, id)))
}

#[tauri::command]
async fn ext_summary(app: St<'_>, drive: Option<String>) -> R<Vec<ExtRow>> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trees = app.trees.read().unwrap();
        analyze::ext_summary(trees.values().filter(|t| drive.as_ref().map_or(true, |d| &t.drive == d)), &app.kb)
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileQuery {
    drive: Option<String>,
    ext: Option<String>,
    cat: Option<String>,
    min_size: u64,
    older_than_days: u32,
    query: Option<String>,
    dirs: bool,
    limit: usize,
}

#[tauri::command]
async fn find_files(app: St<'_>, q: FileQuery) -> R<Vec<NodeLite>> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trees = app.trees.read().unwrap();
        let f = FileFilter {
            ext: q.ext,
            cat: q.cat,
            min_size: q.min_size,
            older_than_days: q.older_than_days,
            query: q.query.filter(|s| !s.trim().is_empty()),
            dirs: q.dirs,
            limit: q.limit.clamp(1, 2000),
        };
        analyze::find_files(trees.values().filter(|t| q.drive.as_ref().map_or(true, |d| &t.drive == d)), &app.kb, &f)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cleanup_report(app: St<'_>) -> R<Vec<CleanGroup>> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trees = app.trees.read().unwrap();
        analyze::cleanup_report(trees.values(), &app.kb)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn leftovers(app: St<'_>) -> R<Vec<Leftover>> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let installed = sys::installed_app_names();
        let mut procs = sys::running_processes();
        procs.iter_mut().for_each(|p| *p = p.trim_end_matches(".exe").to_string());
        let names: Vec<String> = installed.into_iter().chain(procs).collect();
        let trees = app.trees.read().unwrap();
        trees.values().flat_map(|t| analyze::leftovers(t, &app.kb, &names)).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DupProgress {
    done: u64,
    total: u64,
    current: String,
}

#[tauri::command]
async fn start_dupes(app: St<'_>, handle: AppHandle, min_size: u64, personal_only: bool) -> R<()> {
    let app = app.inner().clone();
    let guard = claim(&app, "the duplicate search")?;
    std::thread::spawn(move || {
        let _guard = guard;
        let cands = {
            let trees = app.trees.read().unwrap();
            analyze::dup_candidates(trees.values(), min_size.max(1), personal_only)
        };
        let mut last = std::time::Instant::now();
        let groups = analyze::find_dupes(cands, &app.cancel, |done, total, cur| {
            if last.elapsed().as_millis() > 150 {
                last = std::time::Instant::now();
                let _ = handle.emit("dupes-progress", DupProgress { done, total, current: cur.to_string() });
            }
        });
        let trees = app.trees.read().unwrap();
        let mut out: Vec<DupGroup> = groups
            .into_iter()
            .filter_map(|(size, files)| {
                let files: Vec<NodeLite> =
                    files.into_iter().filter_map(|(d, id)| trees.get(&d).filter(|t| t.valid(id)).map(|t| analyze::lite(t, &app.kb, id, true))).collect();
                (files.len() > 1).then(|| DupGroup { size, wasted: size * (files.len() as u64 - 1), files })
            })
            .collect();
        out.sort_unstable_by(|a, b| b.wasted.cmp(&a.wasted));
        let _ = handle.emit("dupes-done", out);
    });
    Ok(())
}

#[derive(Deserialize)]
struct Target {
    drive: String,
    id: u32,
    mode: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CleanResult {
    path: String,
    mode: String,
    freed: u64,
    moved: u64,
    deleted_files: u64,
    skipped: u64,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CleanProgress {
    index: usize,
    of: usize,
    files: u64,
    current: String,
}

/// Rescans one folder and swaps the fresh result into the tree.
fn rescan_folder(app: &App, drive: &str, id: u32) {
    let Ok(Some((path, name))) = with_tree(app, drive, |t| t.valid(id).then(|| (t.path(id), t.name(id).to_string()))) else { return };
    let Some((mut nodes, names)) = scan::scan_path(&path, &name, false, Arc::new(scan::Progress::default()), Arc::new(AtomicBool::new(false))) else {
        return;
    };
    let mut trees = app.trees.write().unwrap();
    if let Some(t) = trees.get_mut(drive) {
        if t.valid(id) && t.path(id) == path {
            nodes[0].mtime = t.node(id).mtime;
            nodes[0].flags = t.node(id).flags;
            t.graft(id, nodes, names);
        }
    }
}

fn rescan_recycle_bins(app: &App) {
    let targets: Vec<(String, u32)> = {
        let trees = app.trees.read().unwrap();
        trees.values().filter_map(|t| t.find_path(&format!("{}\\$Recycle.Bin", t.drive)).map(|id| (t.drive.clone(), id))).collect()
    };
    for (d, id) in targets {
        rescan_folder(app, &d, id);
    }
}

fn finish_changes(app: &App, drives: &[String]) {
    let mut trees = app.trees.write().unwrap();
    for d in drives {
        if let Some(t) = trees.get_mut(d) {
            scan::refresh_unlisted(t);
        }
    }
    drop(trees);
    let trees = app.trees.read().unwrap();
    for d in drives {
        if let Some(t) = trees.get(d) {
            save_tree(t);
        }
    }
}

#[tauri::command]
async fn clean(app: St<'_>, handle: AppHandle, targets: Vec<Target>) -> R<()> {
    let app = app.inner().clone();
    let guard = claim(&app, "a clean-up")?;
    std::thread::spawn(move || {
        let _guard = guard;
        let mut results = Vec::new();
        let mut touched: Vec<String> = Vec::new();
        let mut any_recycled = false;
        let n = targets.len();
        for (i, tg) in targets.into_iter().enumerate() {
            if app.cancel.load(Relaxed) {
                break;
            }
            let info = with_tree(&app, &tg.drive, |t| {
                if !t.valid(tg.id) {
                    return Err("Already gone.".to_string());
                }
                let info = app.kb.classify(t, tg.id);
                if let Some(why) = analyze::blocked_reason(t, &app.kb, tg.id, &info) {
                    if !(tg.mode == "contents" && info.clean.as_deref() == Some("contents")) {
                        return Err(why);
                    }
                }
                Ok((t.path(tg.id), t.node(tg.id).alloc, t.node(tg.id).is_dir()))
            });
            let (path, before, is_dir) = match info {
                Ok(Ok(x)) => x,
                Ok(Err(e)) | Err(e) => {
                    results.push(CleanResult { path: String::new(), mode: tg.mode, freed: 0, moved: 0, deleted_files: 0, skipped: 0, error: Some(e) });
                    continue;
                }
            };
            let _ = handle.emit("clean-progress", CleanProgress { index: i, of: n, files: 0, current: path.clone() });
            let mut last = std::time::Instant::now();
            let mut count = 0u64;
            let st = clean::delete_path(&path, &tg.mode, &app.cancel, &mut |p| {
                count += 64;
                if last.elapsed().as_millis() > 120 {
                    last = std::time::Instant::now();
                    let _ = handle.emit("clean-progress", CleanProgress { index: i, of: n, files: count, current: p.display().to_string() });
                }
            });
            // Bring the tree up to date with what's really on disk now.
            let gone = std::fs::symlink_metadata(&path).is_err();
            if gone {
                let mut trees = app.trees.write().unwrap();
                if let Some(t) = trees.get_mut(&tg.drive) {
                    t.mark_deleted(tg.id);
                }
            } else if is_dir {
                rescan_folder(&app, &tg.drive, tg.id);
            }
            let after = if gone { 0 } else { with_tree(&app, &tg.drive, |t| if t.valid(tg.id) { t.node(tg.id).alloc } else { 0 }).unwrap_or(0) };
            let removed = before.saturating_sub(after);
            let recycled = tg.mode == "recycle";
            any_recycled |= recycled && gone;
            if !touched.contains(&tg.drive) {
                touched.push(tg.drive.clone());
            }
            results.push(CleanResult {
                path,
                mode: tg.mode,
                freed: if recycled { 0 } else { removed },
                moved: if recycled { removed } else { 0 },
                deleted_files: st.files,
                skipped: st.failed,
                error: st.first_error,
            });
        }
        if any_recycled {
            rescan_recycle_bins(&app);
        }
        finish_changes(&app, &touched);
        let _ = handle.emit("clean-done", results);
    });
    Ok(())
}

#[tauri::command]
async fn run_action(app: St<'_>, action: String, drive: Option<String>) -> R<String> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let msg = clean::run_action(&action, drive.as_deref())?;
        let drives: Vec<String> = app.trees.read().unwrap().keys().cloned().collect();
        match action.as_str() {
            "empty-recycle-bin" => rescan_recycle_bins(&app),
            "hibernate-off" => {
                let mut trees = app.trees.write().unwrap();
                for t in trees.values_mut() {
                    if let Some(id) = t.find_path(&format!("{}\\hiberfil.sys", t.drive)) {
                        if std::fs::symlink_metadata(t.path(id)).is_err() {
                            t.mark_deleted(id);
                        }
                    }
                }
            }
            "dism-cleanup" => {
                let sysd = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
                let id = with_tree(&app, &sysd, |t| t.find_path(&format!("{sysd}\\Windows\\WinSxS"))).ok().flatten();
                if let Some(id) = id {
                    rescan_folder(&app, &sysd, id);
                }
            }
            _ => return Ok(msg),
        }
        finish_changes(&app, &drives);
        Ok(msg)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn rescan(app: St<'_>, drive: String, id: u32) -> R<()> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        rescan_folder(&app, &drive, id);
        finish_changes(&app, &[drive]);
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_path(path: String, select: bool) -> R<()> {
    sys::open_in_explorer(&path, select);
    Ok(())
}

#[tauri::command]
async fn relaunch_admin(handle: AppHandle) -> R<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if sys::run_as_admin(&exe.to_string_lossy(), "", false) {
        handle.exit(0);
        Ok(())
    } else {
        Err("Administrator permission was declined.".into())
    }
}

#[tauri::command]
async fn ask_ai(app: St<'_>, drive: String, id: u32) -> R<ai::AiAnswer> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = with_tree(&app, &drive, |t| {
            if !t.valid(id) {
                return String::new();
            }
            let n = t.node(id);
            let d = analyze::detail(t, &app.kb, id);
            let age = (tree::now_secs() as u32).saturating_sub(n.newest) / 86400;
            let mut s = format!(
                "Path: {}\nType: {}\nSize on disk: {}\nFiles inside: {}\nLast changed: {} days ago\nHidden: {}\n",
                d.path,
                if n.is_dir() { "folder" } else { "file" },
                fmt_bytes(n.alloc),
                n.count,
                age,
                n.flags & tree::F_HIDDEN != 0
            );
            if n.is_dir() {
                s.push_str("Largest items inside:\n");
                for c in t.children(id).take(25) {
                    s.push_str(&format!("  {}{} — {}\n", t.name(c), if t.node(c).is_dir() { "\\" } else { "" }, fmt_bytes(t.node(c).alloc)));
                }
                s.push_str("File types inside: ");
                s.push_str(&d.exts.iter().take(8).map(|e| format!(".{} ({})", e.key, fmt_bytes(e.alloc))).collect::<Vec<_>>().join(", "));
            }
            s
        })?;
        if ctx.is_empty() {
            return Err("That item no longer exists.".into());
        }
        ai::explain(&ctx)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn recycle_bin_info() -> R<(u64, u64)> {
    Ok(sys::recycle_bin_size(None))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if sys::is_elevated() {
        sys::enable_backup_privileges();
    }
    let app = Arc::new(App {
        trees: RwLock::new(HashMap::new()),
        kb: Kb::load(),
        cancel: Arc::new(AtomicBool::new(false)),
        busy: Mutex::new(None),
        loaded: Mutex::new(false),
    });
    tauri::Builder::default()
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            init,
            drives,
            start_scan,
            cancel,
            treemap,
            children,
            detail,
            locate,
            ext_summary,
            find_files,
            cleanup_report,
            leftovers,
            start_dupes,
            clean,
            run_action,
            rescan,
            open_path,
            relaunch_admin,
            ask_ai,
            recycle_bin_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running SpaceScope");
}

/// Headless scan used by `cargo run --example bench` to test the scanner without the UI.
pub fn bench(drive: &str) {
    if sys::is_elevated() {
        sys::enable_backup_privileges();
    }
    let kb = Kb::load();
    let t0 = std::time::Instant::now();
    let t = scan::scan_drive(drive, false, Arc::new(scan::Progress::default()), Arc::new(AtomicBool::new(false))).expect("scan");
    println!(
        "{drive}: {} files, {} dirs, {} denied, scanned {} of {} used in {:?}",
        t.meta.files,
        t.meta.dirs,
        t.meta.denied,
        fmt_bytes(t.node(0).alloc),
        fmt_bytes(t.meta.total - t.meta.free),
        t0.elapsed()
    );
    for c in t.children(0).take(15) {
        let (s, cat, title) = kb.quick(&t, c);
        println!("  {:>10}  {:<40} {s:<8} {cat:<10} {}", fmt_bytes(t.node(c).alloc), t.name(c), title.unwrap_or_default());
    }
    let t1 = std::time::Instant::now();
    let rep = analyze::cleanup_report(std::iter::once(&t), &kb);
    println!("cleanup report in {:?}:", t1.elapsed());
    for g in rep.iter().take(30) {
        println!("  {:>10}  [{}] {} ({} places)", fmt_bytes(g.total), g.safety, g.title, g.n_entries);
    }
    let t2 = std::time::Instant::now();
    let tm = analyze::treemap(&t, &kb, 0, 3, true);
    println!("treemap in {:?}, {} top children", t2.elapsed(), tm.children.map(|c| c.len()).unwrap_or(0));
    let t3 = std::time::Instant::now();
    t.save(&std::env::temp_dir().join("spacescope-bench.bin")).unwrap();
    println!("saved in {:?}", t3.elapsed());
    let t4 = std::time::Instant::now();
    let l = Tree::load(&std::env::temp_dir().join("spacescope-bench.bin")).unwrap();
    println!("loaded {} nodes in {:?}", l.nodes.len(), t4.elapsed());
}

/// Safety self-test for the delete path, run with `cargo run --release --example selftest`.
pub fn selftest() {
    use std::fs;
    let base = std::env::temp_dir().join("spacescope-selftest");
    let _ = fs::remove_dir_all(&base);
    let victim = base.join("victim");
    let outside = base.join("outside-keep");
    fs::create_dir_all(victim.join("a").join("b")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("precious.txt"), "must survive").unwrap();
    for i in 0..50 {
        fs::write(victim.join("a").join(format!("f{i}.tmp")), vec![7u8; 10_000]).unwrap();
    }
    fs::write(victim.join("a").join("b").join("deep.bin"), vec![1u8; 200_000]).unwrap();
    let ro = victim.join("readonly.txt");
    fs::write(&ro, "ro").unwrap();
    let mut perm = fs::metadata(&ro).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(&ro, perm).unwrap();
    // A junction inside the folder pointing outside it: deleting must NOT follow it.
    let st = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", &victim.join("junction").to_string_lossy(), &outside.to_string_lossy()])
        .output()
        .unwrap();
    assert!(st.status.success(), "mklink failed");
    let locked_path = victim.join("locked.dat");
    fs::write(&locked_path, vec![0u8; 5000]).unwrap();
    use std::os::windows::fs::OpenOptionsExt;
    let _lock = fs::OpenOptions::new().read(true).share_mode(0).open(&locked_path).unwrap();

    let vp = victim.to_string_lossy().to_string();
    let (nodes, names) = scan::scan_path(&vp, "victim", false, Arc::new(scan::Progress::default()), Arc::new(AtomicBool::new(false))).unwrap();
    let files = nodes.iter().filter(|n| !n.is_dir()).count();
    let junction_followed = nodes.iter().enumerate().any(|(i, n)| {
        let nm = &names[n.name_off as usize..n.name_off as usize + n.name_len as usize];
        i > 0 && nm == "precious.txt"
    });
    println!("scan: {} files, {} bytes alloc, junction followed: {junction_followed}", files, nodes[0].alloc);
    assert!(!junction_followed, "scanner followed a junction");
    assert_eq!(files, 53, "expected 50 tmp + deep + readonly + locked");

    let mut count = 0;
    let s = clean::delete_path(&vp, "contents", &AtomicBool::new(false), &mut |_| count += 1);
    println!("contents delete: {} files removed, {} failed, first error: {:?}", s.files, s.failed, s.first_error);
    assert!(outside.join("precious.txt").exists(), "DATA LOSS: junction target was deleted");
    assert!(victim.exists(), "root folder should be kept in contents mode");
    assert!(!ro.exists(), "read-only file should be removed");
    assert!(locked_path.exists(), "locked file should be skipped, not crash");
    assert!(!victim.join("junction").exists(), "junction link itself should be removed");
    drop(_lock);
    let s = clean::delete_path(&vp, "permanent", &AtomicBool::new(false), &mut |_| {});
    println!("permanent delete: {} files removed, {} failed", s.files, s.failed);
    assert!(!victim.exists(), "victim should be gone");
    assert!(outside.join("precious.txt").exists(), "DATA LOSS on permanent delete");

    // Recycle Bin round trip
    let r = base.join("recycle-me.txt");
    fs::write(&r, "bye").unwrap();
    sys::recycle(&r.to_string_lossy()).unwrap();
    assert!(!r.exists(), "recycled file should be gone from its folder");
    println!("recycle: ok");

    // Tree bookkeeping: graft + delete keep totals consistent
    let kb = Kb::load();
    let t = scan::scan_drive("C:", true, Arc::new(scan::Progress::default()), Arc::new(AtomicBool::new(false)));
    if let Some(mut t) = t {
        let tmp = t.find_path(&std::env::temp_dir().to_string_lossy()).expect("temp in tree");
        let before = t.node(0).alloc;
        let info = kb.classify(&t, tmp);
        println!("temp dir classified as: {} [{}] blocked={:?}", info.title, info.safety, analyze::blocked_reason(&t, &kb, tmp, &info));
        let (n2, s2) = scan::scan_path(&t.path(tmp), t.name(tmp), false, Arc::new(scan::Progress::default()), Arc::new(AtomicBool::new(false))).unwrap();
        let sub_alloc = n2[0].alloc;
        let old_alloc = t.node(tmp).alloc;
        t.graft(tmp, n2, s2);
        assert_eq!(t.node(tmp).alloc, sub_alloc);
        assert_eq!(t.node(0).alloc, before - old_alloc + sub_alloc);
        let mut stack = vec![0u32];
        let mut sum_files = 0u64;
        while let Some(c) = stack.pop() {
            if t.node(c).is_dir() {
                stack.extend(t.children(c));
            } else {
                sum_files += t.node(c).alloc;
            }
        }
        assert_eq!(sum_files, t.node(0).alloc, "children don't add up to root after graft");
        println!("graft bookkeeping: ok");
    }
    let _ = fs::remove_dir_all(&base);
    println!("ALL SELF-TESTS PASSED");
}
