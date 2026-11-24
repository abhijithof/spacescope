//! Read-only queries over scanned trees: treemap data, details, file types, big files,
//! clean-up recommendations, leftover detection and duplicate finding.

use crate::kb::{Info, Kb};
use crate::tree::*;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeLite {
    pub drive: String,
    pub id: u32,
    pub name: String,
    pub dir: bool,
    pub alloc: u64,
    pub size: u64,
    pub count: u32,
    pub newest: u32,
    pub mtime: u32,
    pub flags: u16,
    pub safety: String,
    pub cat: String,
    pub title: Option<String>,
    pub tcat: String,
    pub path: Option<String>,
}

pub fn lite(t: &Tree, kb: &Kb, id: u32, with_path: bool) -> NodeLite {
    let n = t.node(id);
    let (safety, cat, title) = kb.quick(t, id);
    NodeLite {
        drive: t.drive.clone(),
        id,
        name: if id == 0 { format!("{}\\", t.name(0)) } else { t.name(id).to_string() },
        dir: n.is_dir(),
        alloc: n.alloc,
        size: n.size,
        count: n.count,
        newest: n.newest,
        mtime: n.mtime,
        flags: n.flags,
        safety,
        cat,
        title,
        tcat: if n.is_dir() { "folder".into() } else { kb.ext_cat(t.name(id)).to_string() },
        path: if with_path { Some(t.path(id)) } else { None },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tm {
    pub id: i64,
    pub name: String,
    pub dir: bool,
    pub alloc: u64,
    pub count: u32,
    pub newest: u32,
    pub flags: u16,
    pub safety: String,
    pub cat: String,
    pub title: Option<String>,
    pub tcat: String,
    pub other: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Tm>>,
}

fn dominant_type(t: &Tree, kb: &Kb, id: u32) -> String {
    let mut by: HashMap<&str, u64> = HashMap::new();
    let mut stack = vec![id];
    let mut visited = 0usize;
    while let Some(c) = stack.pop() {
        visited += 1;
        if visited > 400_000 {
            break;
        }
        let n = t.node(c);
        if n.is_dir() {
            stack.extend(t.children(c));
        } else if n.alloc > 0 {
            *by.entry(kb.ext_cat(t.name(c))).or_default() += n.alloc;
        }
    }
    by.into_iter().max_by_key(|x| x.1).map(|x| x.0.to_string()).unwrap_or_else(|| "other".into())
}

pub fn treemap(t: &Tree, kb: &Kb, root: u32, depth: u32, by_type: bool) -> Tm {
    let thresh = (t.node(root).alloc / 3000).max(1);
    fn build(t: &Tree, kb: &Kb, id: u32, d: u32, depth: u32, thresh: u64, by_type: bool) -> Tm {
        let n = t.node(id);
        let (safety, cat, title) = kb.quick(t, id);
        let mut tm = Tm {
            id: id as i64,
            name: if id == 0 { format!("{}\\", t.name(0)) } else { t.name(id).to_string() },
            dir: n.is_dir(),
            alloc: n.alloc,
            count: n.count,
            newest: n.newest,
            flags: n.flags,
            safety,
            cat,
            title,
            tcat: if n.is_dir() { "folder".into() } else { kb.ext_cat(t.name(id)).to_string() },
            other: 0,
            children: None,
        };
        if n.is_dir() && d < depth {
            let mut kids = Vec::new();
            let (mut rest, mut nrest) = (0u64, 0u32);
            for c in t.children(id) {
                let a = t.node(c).alloc;
                if a == 0 {
                    continue;
                }
                if a >= thresh && kids.len() < 250 {
                    kids.push(build(t, kb, c, d + 1, depth, thresh, by_type));
                } else {
                    rest += a;
                    nrest += 1;
                }
            }
            if rest > 0 {
                kids.push(Tm {
                    id: -1,
                    name: format!("{nrest} smaller item{}", if nrest == 1 { "" } else { "s" }),
                    dir: false,
                    alloc: rest,
                    count: nrest,
                    newest: 0,
                    flags: 0,
                    safety: "mixed".into(),
                    cat: "mixed".into(),
                    title: None,
                    tcat: "mixed".into(),
                    other: nrest,
                    children: None,
                });
            }
            if !kids.is_empty() {
                tm.children = Some(kids);
            }
        }
        if by_type && n.is_dir() && tm.children.is_none() {
            tm.tcat = dominant_type(t, kb, id);
        }
        tm
    }
    build(t, kb, root, 0, depth, thresh, by_type)
}

pub fn children(t: &Tree, kb: &Kb, id: u32, limit: usize) -> Vec<NodeLite> {
    t.children(id).filter(|&c| t.node(c).alloc > 0 || t.node(c).is_dir()).take(limit).map(|c| lite(t, kb, c, true)).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub key: String,
    pub label: String,
    pub alloc: u64,
    pub count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub node: NodeLite,
    pub path: String,
    pub info: Info,
    pub crumbs: Vec<(u32, String)>,
    pub types: Vec<Share>,
    pub exts: Vec<Share>,
    pub largest: Vec<NodeLite>,
    pub running: Vec<String>,
    pub blocked: Option<String>,
    pub hidden_files: u64,
}

pub fn detail(t: &Tree, kb: &Kb, id: u32) -> Detail {
    let node = lite(t, kb, id, false);
    let info = kb.classify(t, id);
    let chain = t.chain(id);
    let crumbs = chain.iter().map(|&c| (c, if c == 0 { format!("{}\\", t.name(0)) } else { t.name(c).to_string() })).collect();
    let mut types: HashMap<&str, (u64, u64)> = HashMap::new();
    let mut exts: HashMap<String, (u64, u64)> = HashMap::new();
    let mut largest: Vec<(u64, u32)> = Vec::new();
    let mut hidden = 0u64;
    if t.node(id).is_dir() {
        let mut stack = vec![id];
        while let Some(c) = stack.pop() {
            let n = t.node(c);
            if n.is_dir() {
                stack.extend(t.children(c));
                continue;
            }
            if n.flags & F_SYNTH != 0 {
                continue;
            }
            let name = t.name(c);
            let e = types.entry(kb.ext_cat(name)).or_default();
            e.0 += n.alloc;
            e.1 += 1;
            let ext = ext_of(name).map(|x| x.to_lowercase()).unwrap_or_else(|| "(no extension)".into());
            let e = exts.entry(ext).or_default();
            e.0 += n.alloc;
            e.1 += 1;
            if n.flags & F_HIDDEN != 0 {
                hidden += 1;
            }
            if largest.len() < 8 || n.alloc > largest[largest.len() - 1].0 {
                largest.push((n.alloc, c));
                largest.sort_unstable_by(|a, b| b.0.cmp(&a.0));
                largest.truncate(8);
            }
        }
    }
    let mut types: Vec<Share> = types.into_iter().map(|(k, v)| Share { key: k.into(), label: k.into(), alloc: v.0, count: v.1 }).collect();
    types.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    let mut exts: Vec<Share> = exts
        .into_iter()
        .map(|(k, v)| Share { label: kb.exts.get(&k).map(|d| d.0.clone()).unwrap_or_default(), key: k, alloc: v.0, count: v.1 })
        .collect();
    exts.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    exts.truncate(10);
    let procs = crate::sys::running_processes();
    let running = info.apps.iter().filter(|a| procs.contains(&a.to_lowercase())).cloned().collect();
    Detail {
        path: t.path(id),
        blocked: blocked_reason(t, kb, id, &info),
        node,
        crumbs,
        types,
        exts,
        largest: largest.into_iter().filter(|x| x.1 != id).map(|(_, c)| lite(t, kb, c, true)).collect(),
        running,
        info,
        hidden_files: hidden,
    }
}

/// Rule ids whose matched folder itself must never be deleted (contents may be, if safe).
const PROTECT_SELF: &[&str] = &[
    "drive", "windows", "system32", "program-files", "programdata", "users", "profile", "profile-public", "profile-default", "docs",
    "downloads", "onedrive", "appdata", "user-temp", "win-temp", "programs-user", "store-pkgs", "recycle", "sysvol", "winsxs",
    "win-installer", "windowsapps", "common-files", "recovery", "pagefile", "swapfile", "hiberfil", "ntuser",
];

/// Why this item can't be deleted directly, or None if deletion is allowed (perhaps with a warning).
pub fn blocked_reason(t: &Tree, _kb: &Kb, id: u32, info: &Info) -> Option<String> {
    let n = t.node(id);
    if id == 0 {
        return Some("This is the whole drive.".into());
    }
    if n.flags & F_SYNTH != 0 {
        return Some("This isn't a real file — it's space only Windows can manage.".into());
    }
    if let Some(r) = &info.rule {
        if !info.inherited && PROTECT_SELF.contains(&r.as_str()) {
            return Some(match info.clean.as_deref() {
                Some("contents") => "This folder itself must stay, but you can clear what's inside it.".into(),
                Some(a) if a.starts_with("action:") => "Windows manages this. Use the dedicated action instead of deleting it.".into(),
                _ => "Windows or your user account depends on this folder existing. Look inside it for things to remove.".into(),
            });
        }
    }
    if info.safety == "system" {
        return Some("Part of Windows or an installed program. Deleting it by hand can break your PC — use the suggested action instead.".into());
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtRow {
    pub ext: String,
    pub alloc: u64,
    pub count: u64,
    pub name: String,
    pub cat: String,
    pub safety: String,
    pub what: String,
}

pub fn ext_summary<'a>(trees: impl Iterator<Item = &'a Tree>, kb: &Kb) -> Vec<ExtRow> {
    let mut m: HashMap<String, (u64, u64)> = HashMap::new();
    for t in trees {
        for (i, n) in t.nodes.iter().enumerate() {
            if n.is_dir() || n.deleted() || n.flags & F_SYNTH != 0 {
                continue;
            }
            let name = t.name(i as u32);
            let ext = ext_of(name).map(|x| x.to_lowercase()).unwrap_or_default();
            let e = m.entry(ext).or_default();
            e.0 += n.alloc;
            e.1 += 1;
        }
    }
    let mut rows: Vec<ExtRow> = m
        .into_iter()
        .map(|(ext, (alloc, count))| {
            let d = kb.exts.get(&ext);
            ExtRow {
                name: d.map(|d| d.0.clone()).unwrap_or_else(|| if ext.is_empty() { "No extension".into() } else { format!(".{ext} file") }),
                cat: d.map(|d| d.1.clone()).unwrap_or_else(|| "other".into()),
                safety: d.map(|d| d.2.clone()).unwrap_or_else(|| "unknown".into()),
                what: d.map(|d| d.3.clone()).unwrap_or_default(),
                ext,
                alloc,
                count,
            }
        })
        .collect();
    rows.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    rows.truncate(500);
    rows
}

pub struct FileFilter {
    pub ext: Option<String>,
    pub cat: Option<String>,
    pub min_size: u64,
    pub older_than_days: u32,
    pub query: Option<String>,
    pub dirs: bool,
    pub limit: usize,
}

pub fn find_files<'a>(trees: impl Iterator<Item = &'a Tree>, kb: &Kb, f: &FileFilter) -> Vec<NodeLite> {
    let cutoff = if f.older_than_days > 0 { now_secs().saturating_sub(f.older_than_days as u64 * 86400) as u32 } else { u32::MAX };
    let q = f.query.as_ref().map(|q| q.to_lowercase());
    let mut hits: Vec<(&Tree, u32)> = Vec::new();
    for t in trees {
        for (i, n) in t.nodes.iter().enumerate() {
            if n.deleted() || n.flags & F_SYNTH != 0 || i == 0 || n.alloc < f.min_size {
                continue;
            }
            if n.is_dir() != f.dirs && q.is_none() {
                continue;
            }
            let ts = if n.is_dir() { n.newest } else { n.mtime };
            if ts > cutoff {
                continue;
            }
            let name = t.name(i as u32);
            if let Some(e) = &f.ext {
                if !ext_of(name).map_or(e.is_empty(), |x| x.eq_ignore_ascii_case(e)) {
                    continue;
                }
            }
            if let Some(c) = &f.cat {
                if kb.ext_cat(name) != c {
                    continue;
                }
            }
            if let Some(q) = &q {
                if !name.to_lowercase().contains(q.as_str()) {
                    continue;
                }
            }
            hits.push((t, i as u32));
        }
    }
    hits.sort_unstable_by(|a, b| b.0.node(b.1).alloc.cmp(&a.0.node(a.1).alloc));
    hits.truncate(f.limit);
    hits.into_iter().map(|(t, i)| lite(t, kb, i, true)).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanEntry {
    pub drive: String,
    pub id: u32,
    pub path: String,
    pub alloc: u64,
    pub count: u32,
    pub newest: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanGroup {
    pub rule: String,
    pub drive: String,
    pub title: String,
    pub what: String,
    pub why: String,
    pub note: String,
    pub safety: String,
    pub cat: String,
    pub clean: Option<String>,
    pub admin: bool,
    pub partial: bool,
    pub running: Vec<String>,
    pub total: u64,
    pub files: u64,
    pub n_entries: usize,
    pub entries: Vec<CleanEntry>,
}

pub fn cleanup_report<'a>(trees: impl Iterator<Item = &'a Tree>, kb: &Kb) -> Vec<CleanGroup> {
    let procs = crate::sys::running_processes();
    // One group per (rule, drive) so each drive gets its own clean-up list.
    let mut groups: HashMap<(usize, String), CleanGroup> = HashMap::new();
    for t in trees {
        for (ri, id) in kb.cleanup_matches(t) {
            let r = &kb.rules[ri].def;
            let n = t.node(id);
            let g = groups.entry((ri, t.drive.clone())).or_insert_with(|| CleanGroup {
                rule: r.id.clone(),
                drive: t.drive.clone(),
                title: if r.title.contains('{') { strip_tpl(&r.title) } else { r.title.clone() },
                what: strip_tpl(&r.what),
                why: strip_tpl(&r.why),
                note: strip_tpl(&r.note),
                safety: r.safety.clone(),
                cat: r.cat.clone(),
                clean: r.clean.clone(),
                admin: r.admin,
                partial: r.partial,
                running: r.apps.iter().filter(|a| procs.contains(&a.to_lowercase())).cloned().collect(),
                total: 0,
                files: 0,
                n_entries: 0,
                entries: vec![],
            });
            g.total += n.alloc;
            g.files += n.count as u64;
            g.n_entries += 1;
            g.entries.push(CleanEntry { drive: t.drive.clone(), id, path: t.path(id), alloc: n.alloc, count: n.count, newest: n.newest });
        }
    }
    let mut out: Vec<CleanGroup> = groups.into_values().filter(|g| g.total >= 1 << 20).collect();
    for g in &mut out {
        g.entries.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
        g.entries.truncate(5000);
    }
    out.sort_unstable_by(|a, b| b.total.cmp(&a.total));
    out
}

/// Turns "{parent} cache" into "App cache" for group headings.
fn strip_tpl(s: &str) -> String {
    let s = s.replace(" ({name})", "").replace("{name} ", "");
    let s = s.replace("{parent} cache", "App caches").replace("{parent} logs", "App logs").replace("{parent} temporary files", "App temporary files");
    s.replace("{parent}", "the app").replace("{name}", "this item")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub drive: String,
    pub id: u32,
    pub name: String,
    pub path: String,
    pub location: String,
    pub alloc: u64,
    pub count: u32,
    pub newest: u32,
    pub confidence: String,
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Folders in AppData / ProgramData / Program Files that don't belong to any installed app.
pub fn leftovers(t: &Tree, kb: &Kb, installed: &[String]) -> Vec<Leftover> {
    let tokens: Vec<String> = installed.iter().map(|s| norm(s)).filter(|s| s.len() >= 3).collect();
    let words: std::collections::HashSet<String> =
        installed.iter().flat_map(|s| s.split(|c: char| !c.is_alphanumeric()).map(|w| w.to_lowercase()).collect::<Vec<_>>()).filter(|w| w.len() >= 4).collect();
    let known = |name: &str| -> bool {
        let f = norm(name);
        if f.len() < 3 || kb.known_folders.contains(&name.to_lowercase()) {
            return true;
        }
        if words.contains(&name.to_lowercase()) {
            return true;
        }
        tokens.iter().any(|t| *t == f || (f.len() >= 4 && t.contains(&f)) || (t.len() >= 5 && f.contains(t.as_str())))
    };
    let sysd = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    if !t.drive.eq_ignore_ascii_case(&sysd) {
        return vec![];
    }
    let mut roots: Vec<(u32, String)> = Vec::new();
    for p in [r"ProgramData", r"Program Files", r"Program Files (x86)"] {
        if let Some(id) = t.find_path(&format!("{}\\{}", t.drive, p)) {
            roots.push((id, p.to_string()));
        }
    }
    if let Some(users) = t.find_path(&format!("{}\\Users", t.drive)) {
        for u in t.children(users) {
            for sub in ["Local", "Roaming", "LocalLow"] {
                if let Some(id) = t.find_path(&format!("{}\\AppData\\{}", t.path(u), sub)) {
                    roots.push((id, format!("{}\\AppData\\{}", t.name(u), sub)));
                }
            }
        }
    }
    let now = now_secs() as u32;
    let mut out = Vec::new();
    for (root, loc) in roots {
        for c in t.children(root) {
            let n = t.node(c);
            if !n.is_dir() || n.alloc < (1 << 20) || n.flags & F_REPARSE != 0 {
                continue;
            }
            let name = t.name(c);
            if known(name) {
                continue;
            }
            if let Some((ri, depth)) = kb.find_rule(t, &t.chain(c)) {
                let r = &kb.rules[ri].def;
                if !r.generic && depth + 1 == t.chain(c).len() {
                    continue;
                }
            }
            let age_days = now.saturating_sub(n.newest) / 86400;
            let confidence = if age_days > 180 { "likely" } else if age_days > 30 { "possible" } else { "active" };
            out.push(Leftover {
                drive: t.drive.clone(),
                id: c,
                name: name.to_string(),
                path: t.path(c),
                location: loc.clone(),
                alloc: n.alloc,
                count: n.count,
                newest: n.newest,
                confidence: confidence.into(),
            });
        }
    }
    out.sort_unstable_by(|a, b| b.alloc.cmp(&a.alloc));
    out
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    pub size: u64,
    pub wasted: u64,
    pub files: Vec<NodeLite>,
}

pub struct DupCandidate {
    pub drive: String,
    pub id: u32,
    pub path: String,
    pub size: u64,
}

/// Skip places where duplicates are expected and not yours to remove.
pub fn dup_candidates<'a>(trees: impl Iterator<Item = &'a Tree>, min_size: u64, personal_only: bool) -> Vec<DupCandidate> {
    let mut out = Vec::new();
    for t in trees {
        let mut stack: Vec<u32> = vec![0];
        while let Some(c) = stack.pop() {
            let n = t.node(c);
            if n.is_dir() {
                if n.alloc < min_size * 2 {
                    continue;
                }
                let lname = t.name(c).to_lowercase();
                let parent_is_root = n.parent == 0;
                let skip_always = matches!(lname.as_str(), "$recycle.bin" | "system volume information" | "node_modules" | ".git" | "winsxs" | "windowsapps")
                    || (parent_is_root && (lname == "windows" || lname == "$windows.~bt"));
                let skip_personal = personal_only
                    && ((parent_is_root && matches!(lname.as_str(), "program files" | "program files (x86)" | "programdata" | "windows" | "recovery"))
                        || lname == "appdata"
                        || lname == "target"
                        || lname == ".venv"
                        || lname == "site-packages"
                        || lname.starts_with('.'));
                if skip_always || skip_personal || n.flags & F_REPARSE != 0 {
                    continue;
                }
                stack.extend(t.children(c));
            } else if n.size >= min_size && n.flags & (F_SYNTH | F_HARDLINK | F_CLOUD) == 0 && n.alloc > 0 {
                out.push(DupCandidate { drive: t.drive.clone(), id: c, path: t.path(c), size: n.size });
            }
        }
    }
    out
}

fn hash_file(path: &str, partial: bool) -> Option<u128> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; 1 << 20];
    if partial {
        let len = f.metadata().ok()?.len();
        let chunk = 64 * 1024;
        let n = f.read(&mut buf[..chunk]).ok()?;
        h.update(&buf[..n]);
        if len > chunk as u64 * 2 {
            f.seek(SeekFrom::Start(len / 2)).ok()?;
            let n = f.read(&mut buf[..chunk]).ok()?;
            h.update(&buf[..n]);
            f.seek(SeekFrom::End(-(chunk as i64))).ok()?;
            let n = f.read(&mut buf[..chunk]).ok()?;
            h.update(&buf[..n]);
        }
    } else {
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
    }
    Some(h.digest128())
}

/// Groups by size, then a sampled hash, then a full content hash — only files that are
/// byte-for-byte identical end up in a group.
pub fn find_dupes(cands: Vec<DupCandidate>, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64, &str)) -> Vec<(u64, Vec<(String, u32)>)> {
    let mut by_size: HashMap<u64, Vec<DupCandidate>> = HashMap::new();
    for c in cands {
        by_size.entry(c.size).or_default().push(c);
    }
    let groups: Vec<Vec<DupCandidate>> = by_size.into_values().filter(|v| v.len() > 1).collect();
    let total_bytes: u64 = groups.iter().map(|g| g.iter().map(|c| c.size.min(192 * 1024)).sum::<u64>()).sum();
    let mut done = 0u64;
    let mut out = Vec::new();
    for g in groups {
        if cancel.load(Relaxed) {
            break;
        }
        let mut by_part: HashMap<u128, Vec<&DupCandidate>> = HashMap::new();
        for c in &g {
            done += c.size.min(192 * 1024);
            progress(done, total_bytes, &c.path);
            if let Some(h) = hash_file(&c.path, true) {
                by_part.entry(h).or_default().push(c);
            }
        }
        for (_, pg) in by_part.into_iter().filter(|x| x.1.len() > 1) {
            let mut by_full: HashMap<u128, Vec<&DupCandidate>> = HashMap::new();
            for c in pg {
                if cancel.load(Relaxed) {
                    break;
                }
                progress(done, total_bytes, &c.path);
                if let Some(h) = hash_file(&c.path, false) {
                    by_full.entry(h).or_default().push(c);
                }
            }
            for (_, fg) in by_full.into_iter().filter(|x| x.1.len() > 1) {
                out.push((g[0].size, fg.iter().map(|c| (c.drive.clone(), c.id)).collect()));
            }
        }
    }
    out
}
