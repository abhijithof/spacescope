//! The knowledge base: path rules ("this folder is the Chrome cache, safe to clear") and
//! file-type descriptions. Rules are matched against a node's path segments; the deepest
//! matching ancestor wins, so a file inside the Chrome cache inherits "safe to delete".

use crate::tree::{ext_of, Tree, F_DIR, F_SYNTH};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Deserialize, Clone)]
pub struct RuleDef {
    pub id: String,
    pub paths: Vec<String>,
    pub title: String,
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub why: String,
    pub safety: String,
    pub cat: String,
    /// none | contents | delete | recycle | action:<name>
    #[serde(default)]
    pub clean: Option<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub cleanup: bool,
    #[serde(default)]
    pub apps: Vec<String>,
    /// "dir" or "file"
    #[serde(default)]
    pub only: Option<String>,
    /// Only matches if the parent folder also contains this file (e.g. Cargo.toml next to target).
    #[serde(default)]
    pub sibling: Option<String>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default = "yes")]
    pub inherit: bool,
    /// Broad catch-all rules ("some app's data folder") — don't count as "recognised" for leftovers.
    #[serde(default)]
    pub generic: bool,
    /// Only part of the size is actually reclaimable (e.g. WinSxS via DISM).
    #[serde(default)]
    pub partial: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug)]
enum Seg {
    Lit(String),
    Glob(String),
    Any,
    Deep,
}

impl Seg {
    fn matches(&self, s: &str) -> bool {
        match self {
            Seg::Lit(l) => l == s,
            Seg::Glob(g) => glob(g.as_bytes(), s.as_bytes()),
            Seg::Any => true,
            Seg::Deep => true,
        }
    }
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|k| glob(&p[1..], &s[k..])),
        Some(b'?') => !s.is_empty() && glob(&p[1..], &s[1..]),
        Some(&c) => !s.is_empty() && s[0] == c && glob(&p[1..], &s[1..]),
    }
}

fn match_segs(p: &[Seg], s: &[&str]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(Seg::Deep) => (0..=s.len()).any(|k| match_segs(&p[1..], &s[k..])),
        Some(x) => !s.is_empty() && x.matches(s[0]) && match_segs(&p[1..], &s[1..]),
    }
}

pub struct Rule {
    pub def: RuleDef,
    /// Each pattern with its specificity (literal segments score highest).
    pats: Vec<(Vec<Seg>, i32)>,
}

#[derive(Deserialize, Clone)]
pub struct ExtDef(pub String, pub String, pub String, pub String);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExtView {
    pub ext: String,
    pub name: String,
    pub cat: String,
    pub safety: String,
    pub what: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub rule: Option<String>,
    pub title: String,
    pub what: String,
    pub why: String,
    pub safety: String,
    pub cat: String,
    pub clean: Option<String>,
    pub note: String,
    pub inherited: bool,
    pub matched_id: Option<u32>,
    pub matched_path: Option<String>,
    pub apps: Vec<String>,
    pub admin: bool,
    pub partial: bool,
    pub ext: Option<ExtView>,
    pub source: String,
}

#[derive(Deserialize)]
struct KbFile {
    rules: Vec<RuleDef>,
    known_folders: Vec<String>,
}

pub struct Kb {
    pub rules: Vec<Rule>,
    by_name: HashMap<String, Vec<usize>>,
    by_ext: HashMap<String, Vec<usize>>,
    wild: Vec<usize>,
    pub exts: HashMap<String, ExtDef>,
    pub known_folders: HashSet<String>,
}

fn env_lower(k: &str, dflt: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| dflt.to_string()).to_lowercase().trim_end_matches('\\').to_string()
}

impl Kb {
    pub fn load() -> Kb {
        let file: KbFile = serde_json::from_str(include_str!("../kb/rules.json")).expect("rules.json");
        let exts: HashMap<String, ExtDef> = serde_json::from_str(include_str!("../kb/exts.json")).expect("exts.json");
        let sys = env_lower("SystemDrive", "c:");
        let vars: Vec<(String, String)> = {
            let u = format!("{sys}\\users\\*");
            vec![
                ("{WIN}".into(), env_lower("SystemRoot", "c:\\windows")),
                ("{PF86}".into(), env_lower("ProgramFiles(x86)", "c:\\program files (x86)")),
                ("{PF}".into(), env_lower("ProgramW6432", "c:\\program files")),
                ("{PD}".into(), env_lower("ProgramData", "c:\\programdata")),
                ("{LA}".into(), format!("{u}\\appdata\\local")),
                ("{RA}".into(), format!("{u}\\appdata\\roaming")),
                ("{LL}".into(), format!("{u}\\appdata\\locallow")),
                ("{U}".into(), u.clone()),
                ("{SYS}".into(), sys.clone()),
                ("{DRV}".into(), "*".into()),
            ]
        };
        let mut rules = Vec::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_ext: HashMap<String, Vec<usize>> = HashMap::new();
        let mut wild = Vec::new();
        for def in file.rules {
            let idx = rules.len();
            let mut pats = Vec::new();
            for p in &def.paths {
                let mut s = p.to_lowercase();
                for (k, v) in &vars {
                    s = s.replace(&k.to_lowercase(), v);
                }
                let segs: Vec<Seg> = s
                    .split('\\')
                    .map(|x| match x {
                        "**" => Seg::Deep,
                        "*" => Seg::Any,
                        x if x.contains('*') || x.contains('?') => Seg::Glob(x.to_string()),
                        x => Seg::Lit(x.to_string()),
                    })
                    .collect();
                let sp: i32 = segs
                    .iter()
                    .map(|g| match g {
                        Seg::Lit(_) => 10,
                        Seg::Glob(_) => 6,
                        Seg::Any => 1,
                        Seg::Deep => 0,
                    })
                    .sum();
                match segs.last() {
                    Some(Seg::Lit(l)) => by_name.entry(l.clone()).or_default().push(idx),
                    Some(Seg::Glob(g)) if g.starts_with("*.") && !g[2..].contains(['*', '?']) => {
                        by_ext.entry(g[2..].to_string()).or_default().push(idx)
                    }
                    _ => wild.push(idx),
                }
                pats.push((segs, sp));
            }
            for v in by_name.values_mut().chain(by_ext.values_mut()) {
                v.dedup();
            }
            wild.dedup();
            rules.push(Rule { def, pats });
        }
        Kb {
            rules,
            by_name,
            by_ext,
            wild,
            exts,
            known_folders: file.known_folders.into_iter().map(|s| s.to_lowercase()).collect(),
        }
    }

    pub fn ext_view(&self, name: &str) -> Option<ExtView> {
        let e = ext_of(name)?.to_lowercase();
        self.exts.get(&e).map(|d| ExtView { ext: e.clone(), name: d.0.clone(), cat: d.1.clone(), safety: d.2.clone(), what: d.3.clone() })
    }

    pub fn ext_cat(&self, name: &str) -> &str {
        let Some(e) = ext_of(name) else { return "other" };
        let mut buf = [0u8; 16];
        let b = e.as_bytes();
        if b.len() > 16 {
            return "other";
        }
        for (i, c) in b.iter().enumerate() {
            buf[i] = c.to_ascii_lowercase();
        }
        let key = std::str::from_utf8(&buf[..b.len()]).unwrap_or("");
        self.exts.get(key).map(|d| d.1.as_str()).unwrap_or("other")
    }

    fn candidates<'a>(&'a self, name: &str, is_dir: bool, include_wild: bool) -> impl Iterator<Item = usize> + 'a {
        let a = self.by_name.get(name).into_iter().flatten().copied();
        let b = if is_dir { None } else { ext_of(name).and_then(|e| self.by_ext.get(e)) };
        let c = if include_wild { Some(&self.wild) } else { None };
        a.chain(b.into_iter().flatten().copied()).chain(c.into_iter().flatten().copied())
    }

    /// Returns the specificity of the best pattern of `r` matching `segs`, if any.
    fn rule_ok(&self, r: &Rule, segs: &[&str], is_dir: bool, sibling_ok: impl Fn(&str) -> bool) -> Option<i32> {
        match r.def.only.as_deref() {
            Some("dir") if !is_dir => return None,
            Some("file") if is_dir => return None,
            _ => {}
        }
        let spec = r.pats.iter().filter(|p| match_segs(&p.0, segs)).map(|p| p.1).max()?;
        match &r.def.sibling {
            Some(s) if !sibling_ok(s) => None,
            _ => Some(spec),
        }
    }

    /// Finds the most specific rule matching the node at the end of `chain` or any ancestor.
    /// Returns (rule index, depth in chain).
    pub fn find_rule(&self, t: &Tree, chain: &[u32]) -> Option<(usize, usize)> {
        let lower: Vec<String> = chain.iter().map(|&c| t.name(c).to_lowercase()).collect();
        let segs: Vec<&str> = lower.iter().map(|s| s.as_str()).collect();
        for d in (1..=chain.len()).rev() {
            let id = chain[d - 1];
            let is_dir = t.node(id).flags & F_DIR != 0;
            let inherited = d < chain.len();
            let sib = |name: &str| d >= 2 && t.children(chain[d - 2]).any(|c| t.name(c).eq_ignore_ascii_case(name));
            let mut best: Option<(usize, i32)> = None;
            for ri in self.candidates(&segs[d - 1], is_dir, true) {
                let r = &self.rules[ri];
                if inherited && !r.def.inherit {
                    continue;
                }
                if let Some(sp) = self.rule_ok(r, &segs[..d], is_dir, &sib) {
                    if best.map_or(true, |b| sp > b.1) {
                        best = Some((ri, sp));
                    }
                }
            }
            if let Some((b, _)) = best {
                return Some((b, d - 1));
            }
        }
        None
    }

    pub fn classify(&self, t: &Tree, id: u32) -> Info {
        let nd = t.node(id);
        if nd.flags & F_SYNTH != 0 {
            return self.unlisted_info(t);
        }
        let chain = t.chain(id);
        let ext = if nd.is_dir() { None } else { self.ext_view(t.name(id)) };
        if let Some((ri, depth)) = self.find_rule(t, &chain) {
            let r = &self.rules[ri].def;
            let mid = chain[depth];
            let name = t.name(mid);
            let parent = if depth > 0 { t.name(chain[depth - 1]) } else { "" };
            let fill = |s: &str| s.replace("{name}", name).replace("{parent}", parent);
            let inherited = depth + 1 < chain.len();
            let mut info = Info {
                rule: Some(r.id.clone()),
                title: fill(&r.title),
                what: fill(&r.what),
                why: fill(&r.why),
                safety: r.safety.clone(),
                cat: r.cat.clone(),
                clean: r.clean.clone(),
                note: fill(&r.note),
                inherited,
                matched_id: Some(mid),
                matched_path: Some(t.path(mid)),
                apps: r.apps.clone(),
                admin: r.admin,
                partial: r.partial,
                ext,
                source: "rule".into(),
            };
            // A user file inside a generic location is better described by its type.
            if inherited && r.generic {
                if let Some(e) = &info.ext {
                    info.safety = if r.safety == "system" { r.safety.clone() } else { e.safety.clone() };
                }
            }
            return info;
        }
        if let Some(e) = ext {
            return Info {
                title: e.name.clone(),
                what: e.what.clone(),
                safety: e.safety.clone(),
                cat: e.cat.clone(),
                ext: Some(e),
                source: "ext".into(),
                ..Default::default()
            };
        }
        Info { title: String::new(), safety: "unknown".into(), cat: "unknown".into(), source: "none".into(), ..Default::default() }
    }

    /// Safety + category only — cheaper than `classify` for colouring many treemap cells.
    pub fn quick(&self, t: &Tree, id: u32) -> (String, String, Option<String>) {
        let nd = t.node(id);
        if nd.flags & F_SYNTH != 0 {
            return ("system".into(), "system".into(), Some("Unlisted system space".into()));
        }
        let chain = t.chain(id);
        if let Some((ri, depth)) = self.find_rule(t, &chain) {
            let r = &self.rules[ri].def;
            let inherited = depth + 1 < chain.len();
            let mut safety = r.safety.clone();
            if inherited && r.generic && !nd.is_dir() {
                if let Some(e) = self.ext_view(t.name(id)) {
                    if r.safety != "system" {
                        safety = e.safety;
                    }
                }
            }
            let title = if inherited { None } else { Some(r.title.replace("{name}", t.name(id)).replace("{parent}", if depth > 0 { t.name(chain[depth - 1]) } else { "" })) };
            return (safety, r.cat.clone(), title);
        }
        if !nd.is_dir() {
            if let Some(e) = self.ext_view(t.name(id)) {
                return (e.safety, e.cat, None);
            }
        }
        ("unknown".into(), "unknown".into(), None)
    }

    fn unlisted_info(&self, t: &Tree) -> Info {
        let admin = t.meta.elevated;
        Info {
            rule: Some("unlisted".into()),
            title: "Unlisted system space".into(),
            what: if admin {
                "Space the drive reports as used that doesn't belong to any file a folder listing can see. On NTFS this is mostly the file system's own bookkeeping (the Master File Table, the change journal, security descriptors) plus Volume Shadow Copies used by System Restore.".into()
            } else {
                "Space the drive reports as used that doesn't belong to any file SpaceScope could list. Part of it is NTFS bookkeeping (Master File Table, change journal), part is System Restore snapshots, and part is folders Windows wouldn't let us read without administrator rights.".into()
            },
            why: "Windows needs the file-system metadata to function. Shadow copies are your System Restore points.".into(),
            safety: "system".into(),
            cat: "system".into(),
            clean: Some("action:restore-points".into()),
            note: if admin {
                "If this is large, it is usually System Restore. Open System Protection → Configure to lower the maximum usage or delete old restore points.".into()
            } else {
                "Restart SpaceScope as administrator to see inside locked folders — this number will shrink to the true system overhead.".into()
            },
            source: "rule".into(),
            ..Default::default()
        }
    }

    /// Walks the whole tree once and returns every top-most node matched by a cleanup rule.
    pub fn cleanup_matches(&self, t: &Tree) -> Vec<(usize, u32)> {
        let mut out = Vec::new();
        // stack of (node, depth); segs holds lower-cased names along the current path
        let mut segs: Vec<String> = vec![t.name(0).to_lowercase()];
        let mut stack: Vec<(u32, usize)> = t.children(0).map(|c| (c, 1)).collect();
        while let Some((id, depth)) = stack.pop() {
            segs.truncate(depth);
            let nd = t.node(id);
            if nd.flags & F_SYNTH != 0 || nd.alloc == 0 {
                continue;
            }
            let lname = t.name(id).to_lowercase();
            let is_dir = nd.is_dir();
            segs.push(lname);
            let refs: Vec<&str> = segs.iter().map(|s| s.as_str()).collect();
            let parent = nd.parent;
            let sib = |name: &str| t.children(parent).any(|c| t.name(c).eq_ignore_ascii_case(name));
            let mut best: Option<(usize, i32)> = None;
            for ri in self.candidates(&segs[depth], is_dir, is_dir) {
                let r = &self.rules[ri];
                if !r.def.cleanup {
                    continue;
                }
                if let Some(sp) = self.rule_ok(r, &refs, is_dir, &sib) {
                    if best.map_or(true, |b| sp > b.1) {
                        best = Some((ri, sp));
                    }
                }
            }
            if let Some((b, _)) = best {
                out.push((b, id));
                continue;
            }
            if is_dir {
                for c in t.children(id) {
                    stack.push((c, depth + 1));
                }
            }
        }
        out
    }
}
