//! Compact in-memory file tree. Every file and folder is one 40-byte `Node` in a flat
//! arena; names live in one shared string pool. A parent always has a lower index than
//! its children, which lets sizes be rolled up in a single reverse pass.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub const F_DIR: u16 = 1;
pub const F_HIDDEN: u16 = 2;
pub const F_SYSTEM: u16 = 4;
pub const F_REPARSE: u16 = 8;
pub const F_DENIED: u16 = 16;
pub const F_DELETED: u16 = 32;
pub const F_SYNTH: u16 = 64;
pub const F_HARDLINK: u16 = 128;
pub const F_CLOUD: u16 = 256;
pub const F_COMPRESSED: u16 = 512;

pub const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Node {
    pub parent: u32,
    pub name_off: u32,
    pub name_len: u16,
    pub flags: u16,
    /// Last write time, unix seconds.
    pub mtime: u32,
    /// Logical size in bytes (rolled up for folders).
    pub size: u64,
    /// Bytes actually allocated on disk (rolled up for folders) — this is what deleting frees.
    pub alloc: u64,
    /// Number of files in the subtree (1 for a file).
    pub count: u32,
    /// Newest modification time anywhere in the subtree.
    pub newest: u32,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    pub scanned_at: u64,
    pub duration_ms: u64,
    pub total: u64,
    pub free: u64,
    pub fs: String,
    pub label: String,
    pub elevated: bool,
    pub denied: u32,
    pub files: u64,
    pub dirs: u64,
}

pub struct Tree {
    pub drive: String,
    pub nodes: Vec<Node>,
    pub names: String,
    pub child_start: Vec<u32>,
    pub child_list: Vec<u32>,
    pub meta: ScanMeta,
}

impl Node {
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.flags & F_DIR != 0
    }
    #[inline]
    pub fn deleted(&self) -> bool {
        self.flags & F_DELETED != 0
    }
}

/// Rolls file sizes, counts and newest-times up into their folders.
pub fn aggregate(nodes: &mut [Node]) {
    for i in (1..nodes.len()).rev() {
        let c = nodes[i];
        if c.deleted() || c.parent == NONE {
            continue;
        }
        let p = &mut nodes[c.parent as usize];
        p.size += c.size;
        p.alloc += c.alloc;
        p.count += c.count;
        if c.newest > p.newest {
            p.newest = c.newest;
        }
    }
}

impl Tree {
    pub fn new(drive: String, nodes: Vec<Node>, names: String, meta: ScanMeta) -> Tree {
        let mut t = Tree { drive, nodes, names, child_start: vec![], child_list: vec![], meta };
        t.build_index();
        t
    }

    #[inline]
    pub fn node(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    #[inline]
    pub fn name(&self, i: u32) -> &str {
        let n = &self.nodes[i as usize];
        &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }

    pub fn valid(&self, i: u32) -> bool {
        (i as usize) < self.nodes.len() && !self.nodes[i as usize].deleted()
    }

    pub fn children(&self, i: u32) -> impl Iterator<Item = u32> + '_ {
        let (a, b) = (self.child_start[i as usize] as usize, self.child_start[i as usize + 1] as usize);
        self.child_list[a..b].iter().copied().filter(move |&c| !self.nodes[c as usize].deleted())
    }

    /// Ids from the drive root down to `i` inclusive.
    pub fn chain(&self, i: u32) -> Vec<u32> {
        let mut v = Vec::with_capacity(16);
        let mut c = i;
        while c != NONE {
            v.push(c);
            c = self.nodes[c as usize].parent;
        }
        v.reverse();
        v
    }

    pub fn path(&self, i: u32) -> String {
        let chain = self.chain(i);
        if chain.len() == 1 {
            return format!("{}\\", self.name(chain[0]));
        }
        let parts: Vec<&str> = chain.iter().map(|&c| self.name(c)).collect();
        parts.join("\\")
    }

    /// Finds a node by full path (case-insensitive). Returns None if any segment is missing.
    pub fn find_path(&self, path: &str) -> Option<u32> {
        let mut segs = path.trim_end_matches('\\').split('\\');
        let drive = segs.next()?;
        if !drive.eq_ignore_ascii_case(self.name(0)) {
            return None;
        }
        let mut cur = 0u32;
        for s in segs {
            if s.is_empty() {
                continue;
            }
            cur = self.children(cur).find(|&c| self.name(c).eq_ignore_ascii_case(s) || self.name(c).to_lowercase() == s.to_lowercase())?;
        }
        Some(cur)
    }

    pub fn build_index(&mut self) {
        let n = self.nodes.len();
        let mut start = vec![0u32; n + 1];
        for nd in self.nodes.iter().skip(1) {
            if !nd.deleted() && nd.parent != NONE {
                start[nd.parent as usize + 1] += 1;
            }
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut list = vec![0u32; start[n] as usize];
        for (i, nd) in self.nodes.iter().enumerate().skip(1) {
            if !nd.deleted() && nd.parent != NONE {
                let p = nd.parent as usize;
                list[fill[p] as usize] = i as u32;
                fill[p] += 1;
            }
        }
        let nodes = &self.nodes;
        for i in 0..n {
            let (a, b) = (start[i] as usize, start[i + 1] as usize);
            if b - a > 1 {
                list[a..b].sort_unstable_by(|&x, &y| nodes[y as usize].alloc.cmp(&nodes[x as usize].alloc));
            }
        }
        self.child_start = start;
        self.child_list = list;
    }

    /// Removes `i` and its subtree from the totals of every ancestor and flags the subtree deleted.
    pub fn mark_deleted(&mut self, i: u32) {
        if i == 0 || !self.valid(i) {
            return;
        }
        let nd = self.nodes[i as usize];
        let mut p = nd.parent;
        while p != NONE {
            let a = &mut self.nodes[p as usize];
            a.size = a.size.saturating_sub(nd.size);
            a.alloc = a.alloc.saturating_sub(nd.alloc);
            a.count = a.count.saturating_sub(nd.count);
            p = a.parent;
        }
        let mut stack = vec![i];
        while let Some(c) = stack.pop() {
            let kids: Vec<u32> = self.children(c).collect();
            self.nodes[c as usize].flags |= F_DELETED;
            stack.extend(kids);
        }
    }

    /// Replaces the contents of folder `at` with a freshly scanned subtree whose root is `sub[0]`.
    pub fn graft(&mut self, at: u32, sub_nodes: Vec<Node>, sub_names: String) {
        let old = self.nodes[at as usize];
        let kids: Vec<u32> = self.children(at).collect();
        for k in kids {
            let mut stack = vec![k];
            while let Some(c) = stack.pop() {
                let more: Vec<u32> = self.children(c).collect();
                self.nodes[c as usize].flags |= F_DELETED;
                stack.extend(more);
            }
        }
        let base = self.nodes.len() as u32;
        let name_base = self.names.len() as u32;
        self.names.push_str(&sub_names);
        for n in sub_nodes.iter().skip(1) {
            let mut m = *n;
            m.name_off += name_base;
            m.parent = if n.parent == 0 { at } else { n.parent - 1 + base };
            self.nodes.push(m);
        }
        let r = sub_nodes[0];
        {
            let a = &mut self.nodes[at as usize];
            a.size = r.size;
            a.alloc = r.alloc;
            a.count = r.count;
            a.newest = r.newest.max(a.mtime);
            a.flags = (a.flags & !F_DENIED) | (r.flags & F_DENIED);
        }
        let mut p = old.parent;
        while p != NONE {
            let a = &mut self.nodes[p as usize];
            a.size = (a.size + r.size).saturating_sub(old.size);
            a.alloc = (a.alloc + r.alloc).saturating_sub(old.alloc);
            a.count = (a.count + r.count).saturating_sub(old.count);
            a.newest = a.newest.max(r.newest);
            p = a.parent;
        }
        self.build_index();
    }

    /// Drops deleted nodes from the arena. Only done on load, since it renumbers ids.
    fn compact(&mut self) {
        if !self.nodes.iter().any(|n| n.deleted()) {
            return;
        }
        let mut map = vec![NONE; self.nodes.len()];
        let mut nodes = Vec::with_capacity(self.nodes.len());
        let mut names = String::with_capacity(self.names.len());
        for i in 0..self.nodes.len() {
            let nd = self.nodes[i];
            if nd.deleted() {
                continue;
            }
            let parent = if i == 0 { NONE } else { map[nd.parent as usize] };
            if i != 0 && parent == NONE {
                continue;
            }
            let mut m = nd;
            m.parent = parent;
            m.name_off = names.len() as u32;
            names.push_str(self.name(i as u32));
            map[i] = nodes.len() as u32;
            nodes.push(m);
        }
        self.nodes = nodes;
        self.names = names;
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let meta = serde_json::to_vec(&self.meta).unwrap();
        let mut buf: Vec<u8> = Vec::with_capacity(self.nodes.len() * 40 + self.names.len() + 256);
        buf.extend_from_slice(b"SSCOPE01");
        buf.extend_from_slice(&(self.drive.len() as u32).to_le_bytes());
        buf.extend_from_slice(self.drive.as_bytes());
        buf.extend_from_slice(&(meta.len() as u32).to_le_bytes());
        buf.extend_from_slice(&meta);
        buf.extend_from_slice(&(self.nodes.len() as u64).to_le_bytes());
        // SAFETY: Node is repr(C), Copy and has no padding (40 bytes of plain integers).
        let raw = unsafe { std::slice::from_raw_parts(self.nodes.as_ptr() as *const u8, self.nodes.len() * std::mem::size_of::<Node>()) };
        buf.extend_from_slice(raw);
        buf.extend_from_slice(&(self.names.len() as u64).to_le_bytes());
        buf.extend_from_slice(self.names.as_bytes());
        let packed = lz4_flex::compress_prepend_size(&buf);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&packed)?;
        drop(f);
        std::fs::rename(&tmp, path)
    }

    pub fn load(path: &Path) -> Option<Tree> {
        let packed = std::fs::read(path).ok()?;
        let buf = lz4_flex::decompress_size_prepended(&packed).ok()?;
        if buf.len() < 8 || &buf[..8] != b"SSCOPE01" {
            return None;
        }
        let mut pos = 8usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = buf.get(pos..pos + n)?;
            pos += n;
            Some(s)
        };
        let dl = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let drive = String::from_utf8(take(dl)?.to_vec()).ok()?;
        let ml = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let meta: ScanMeta = serde_json::from_slice(take(ml)?).ok()?;
        let n = u64::from_le_bytes(take(8)?.try_into().ok()?) as usize;
        let raw = take(n * std::mem::size_of::<Node>())?;
        let mut nodes = vec![Node::default(); n];
        // SAFETY: sizes match exactly and Node is plain old data.
        unsafe { std::ptr::copy_nonoverlapping(raw.as_ptr(), nodes.as_mut_ptr() as *mut u8, raw.len()) };
        let sl = u64::from_le_bytes(take(8)?.try_into().ok()?) as usize;
        let names = String::from_utf8(take(sl)?.to_vec()).ok()?;
        if nodes.is_empty() {
            return None;
        }
        let mut t = Tree { drive, nodes, names, child_start: vec![], child_list: vec![], meta };
        t.compact();
        t.build_index();
        Some(t)
    }
}

pub fn ext_of(name: &str) -> Option<&str> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() || name.len() - dot > 16 {
        return None;
    }
    Some(&name[dot + 1..])
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
