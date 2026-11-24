//! Parallel directory scanner built on NtQueryDirectoryFile. One syscall returns a whole
//! batch of entries with logical size, on-disk allocation, attributes and file IDs, so we
//! never open individual files. Hard links (WinSxS <-> System32) are counted once via file IDs.

use crate::sys;
use crate::tree::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

#[repr(C)]
struct IoStatusBlock {
    status: usize,
    info: usize,
}

#[link(name = "ntdll", kind = "raw-dylib")]
extern "system" {
    fn NtQueryDirectoryFile(
        file: *mut c_void,
        event: *mut c_void,
        apc: *mut c_void,
        ctx: *mut c_void,
        iosb: *mut IoStatusBlock,
        buf: *mut c_void,
        len: u32,
        class: u32,
        single: u8,
        mask: *mut c_void,
        restart: u8,
    ) -> i32;
}

const STATUS_NO_MORE_FILES: i32 = 0x8000_0006u32 as i32;
const UNSUPPORTED: [u32; 4] = [0xC000_0003, 0xC000_000D, 0xC000_00BB, 0xC000_0010];
const CLASS_ID_FULL: u32 = 38; // FileIdFullDirectoryInformation (has file IDs; NTFS/ReFS)
const CLASS_BASIC: u32 = 1; // FileDirectoryInformation (works on FAT/exFAT)
const BUF_WORDS: usize = 128 * 1024 / 8;
const SHARDS: usize = 64;

struct Entry {
    name: String,
    attrs: u32,
    size: u64,
    alloc: u64,
    mtime: u32,
    id: u64,
    linked: bool,
}

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
    pub current: Mutex<String>,
}

fn filetime_to_unix(ft: i64) -> u32 {
    const EPOCH: i64 = 116_444_736_000_000_000;
    if ft <= EPOCH {
        0
    } else {
        ((ft - EPOCH) / 10_000_000).min(u32::MAX as i64) as u32
    }
}

fn list_dir(path: &str, ids: &AtomicBool, buf: &mut [u64], out: &mut Vec<Entry>) -> Result<(), u32> {
    let w = sys::wide(&sys::long_path(path));
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(unsafe { GetLastError() });
    }
    let mut class = if ids.load(Relaxed) { CLASS_ID_FULL } else { CLASS_BASIC };
    let mut restart = 1u8;
    let mut result = Ok(());
    loop {
        let mut iosb = IoStatusBlock { status: 0, info: 0 };
        let st = unsafe {
            NtQueryDirectoryFile(
                h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut iosb,
                buf.as_mut_ptr() as *mut c_void,
                (buf.len() * 8) as u32,
                class,
                0,
                std::ptr::null_mut(),
                restart,
            )
        };
        if st == STATUS_NO_MORE_FILES {
            break;
        }
        if st < 0 {
            if class == CLASS_ID_FULL && restart == 1 && UNSUPPORTED.contains(&(st as u32)) {
                class = CLASS_BASIC;
                ids.store(false, Relaxed);
                continue;
            }
            if restart == 1 {
                result = Err(st as u32);
            }
            break;
        }
        restart = 0;
        if iosb.info == 0 {
            break;
        }
        let base = buf.as_ptr() as *const u8;
        let mut off = 0usize;
        loop {
            // SAFETY: the kernel wrote a chain of 8-byte aligned records inside `buf`.
            unsafe {
                let p = base.add(off);
                let r32 = |o: usize| (p.add(o) as *const u32).read_unaligned();
                let r64 = |o: usize| (p.add(o) as *const i64).read_unaligned();
                let next = r32(0) as usize;
                let nlen = r32(60) as usize / 2;
                let (id, noff) = if class == CLASS_ID_FULL { (r64(72) as u64, 80) } else { (0, 64) };
                let n16 = std::slice::from_raw_parts(p.add(noff) as *const u16, nlen);
                let dots = (nlen == 1 && n16[0] == 46) || (nlen == 2 && n16[0] == 46 && n16[1] == 46);
                if !dots {
                    out.push(Entry {
                        name: String::from_utf16_lossy(n16),
                        mtime: filetime_to_unix(r64(24)),
                        size: r64(40).max(0) as u64,
                        alloc: r64(48).max(0) as u64,
                        attrs: r32(56),
                        id,
                        linked: false,
                    });
                }
                if next == 0 {
                    break;
                }
                off += next;
            }
        }
    }
    unsafe { CloseHandle(h) };
    result
}

struct Ctx {
    b: Mutex<(Vec<Node>, String)>,
    prog: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    ids: AtomicBool,
    seen: Vec<Mutex<HashSet<u64>>>,
}

thread_local! {
    static BUF: RefCell<Vec<u64>> = RefCell::new(vec![0u64; BUF_WORDS]);
}

fn flags_of(attrs: u32) -> u16 {
    let mut f = 0;
    if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
        f |= F_DIR;
    }
    if attrs & FILE_ATTRIBUTE_HIDDEN != 0 {
        f |= F_HIDDEN;
    }
    if attrs & FILE_ATTRIBUTE_SYSTEM != 0 {
        f |= F_SYSTEM;
    }
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        f |= F_REPARSE;
    }
    if attrs & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN) != 0 {
        f |= F_CLOUD;
    }
    if attrs & FILE_ATTRIBUTE_COMPRESSED != 0 {
        f |= F_COMPRESSED;
    }
    f
}

fn join(parent: &str, name: &str) -> String {
    if parent.ends_with('\\') {
        format!("{parent}{name}")
    } else {
        format!("{parent}\\{name}")
    }
}

fn process<'s>(ctx: &'s Ctx, s: &rayon::Scope<'s>, idx: u32, path: String) {
    if ctx.cancel.load(Relaxed) {
        return;
    }
    let mut entries = Vec::new();
    let res = BUF.with(|b| list_dir(&path, &ctx.ids, &mut b.borrow_mut(), &mut entries));
    ctx.prog.dirs.fetch_add(1, Relaxed);
    if res.is_err() {
        ctx.prog.denied.fetch_add(1, Relaxed);
        ctx.b.lock().unwrap().0[idx as usize].flags |= F_DENIED;
        return;
    }
    if let Ok(mut c) = ctx.prog.current.try_lock() {
        c.clear();
        c.push_str(&path);
    }
    let mut bytes = 0u64;
    let mut files = 0u64;
    for e in entries.iter_mut() {
        if e.attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
            files += 1;
            if e.id != 0 && e.alloc > 0 {
                let shard = ((e.id ^ (e.id >> 23)) as usize) % SHARDS;
                if !ctx.seen[shard].lock().unwrap().insert(e.id) {
                    e.linked = true;
                    e.alloc = 0;
                }
            }
            bytes += e.alloc;
        }
    }
    ctx.prog.files.fetch_add(files, Relaxed);
    ctx.prog.bytes.fetch_add(bytes, Relaxed);
    let mut subdirs: Vec<(u32, String)> = Vec::new();
    {
        let mut g = ctx.b.lock().unwrap();
        let (nodes, names) = &mut *g;
        for e in &entries {
            let off = names.len() as u32;
            names.push_str(&e.name);
            let mut flags = flags_of(e.attrs);
            if e.linked {
                flags |= F_HARDLINK;
            }
            let dir = flags & F_DIR != 0;
            let i = nodes.len() as u32;
            nodes.push(Node {
                parent: idx,
                name_off: off,
                name_len: e.name.len() as u16,
                flags,
                mtime: e.mtime,
                size: if dir { 0 } else { e.size },
                alloc: if dir { 0 } else { e.alloc },
                count: if dir { 0 } else { 1 },
                newest: e.mtime,
            });
            // Junctions and symlinks point elsewhere on disk; following them would double count.
            if dir && flags & F_REPARSE == 0 {
                subdirs.push((i, e.name.clone()));
            }
        }
    }
    for (ci, name) in subdirs {
        let p = join(&path, &name);
        s.spawn(move |s| process(ctx, s, ci, p));
    }
}

pub fn thread_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(4, 16)
}

/// Scans `root_path` and everything below it. Returns the rolled-up arena (root at index 0),
/// or None when cancelled.
pub fn scan_path(root_path: &str, root_name: &str, gentle: bool, prog: Arc<Progress>, cancel: Arc<AtomicBool>) -> Option<(Vec<Node>, String)> {
    let mut nodes = Vec::with_capacity(1 << 16);
    nodes.push(Node { parent: NONE, name_off: 0, name_len: root_name.len() as u16, flags: F_DIR, ..Default::default() });
    let ctx = Ctx {
        b: Mutex::new((nodes, root_name.to_string())),
        prog,
        cancel: cancel.clone(),
        ids: AtomicBool::new(true),
        seen: (0..SHARDS).map(|_| Mutex::new(HashSet::new())).collect(),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_count())
        .start_handler(move |_| {
            if gentle {
                // Low CPU *and* low I/O priority: the scan yields to anything else you're doing.
                unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
            }
        })
        .build()
        .ok()?;
    let root = root_path.to_string();
    pool.scope(|s| process(&ctx, s, 0, root));
    if cancel.load(Relaxed) {
        return None;
    }
    let (mut nodes, names) = ctx.b.into_inner().unwrap();
    aggregate(&mut nodes);
    Some((nodes, names))
}

pub const UNLISTED_NAME: &str = "Unlisted system space";

pub fn scan_drive(drive: &str, gentle: bool, prog: Arc<Progress>, cancel: Arc<AtomicBool>) -> Option<Tree> {
    let t0 = std::time::Instant::now();
    let root = format!("{drive}\\");
    let (mut nodes, mut names) = scan_path(&root, drive, gentle, prog.clone(), cancel)?;
    let vol = sys::volume_info(&root);
    let used = vol.total.saturating_sub(vol.free);
    if used > nodes[0].alloc + (64 << 20) {
        // Space the volume reports as used but no folder listing can see: NTFS metadata
        // ($MFT, journal), shadow copies, and anything we were denied access to.
        let extra = used - nodes[0].alloc;
        let off = names.len() as u32;
        names.push_str(UNLISTED_NAME);
        nodes.push(Node {
            parent: 0,
            name_off: off,
            name_len: UNLISTED_NAME.len() as u16,
            flags: F_SYNTH,
            mtime: 0,
            size: extra,
            alloc: extra,
            count: 0,
            newest: 0,
        });
        nodes[0].size += extra;
        nodes[0].alloc += extra;
    }
    let meta = ScanMeta {
        scanned_at: now_secs(),
        duration_ms: t0.elapsed().as_millis() as u64,
        total: vol.total,
        free: vol.free,
        fs: vol.fs,
        label: vol.label,
        elevated: sys::is_elevated(),
        denied: prog.denied.load(Relaxed) as u32,
        files: nodes.iter().filter(|n| !n.is_dir() && n.flags & F_SYNTH == 0).count() as u64,
        dirs: nodes.iter().filter(|n| n.is_dir()).count() as u64,
    };
    Some(Tree::new(drive.to_string(), nodes, names, meta))
}

/// Re-reads the space the volume reports as used and resizes the synthetic "unlisted" node.
pub fn refresh_unlisted(t: &mut Tree) {
    let vol = sys::volume_info(&format!("{}\\", t.drive));
    t.meta.free = vol.free;
    let used = vol.total.saturating_sub(vol.free);
    let synth = t.children(0).find(|&c| t.node(c).flags & F_SYNTH != 0);
    if let Some(s) = synth {
        let old = t.nodes[s as usize].alloc;
        let real = t.nodes[0].alloc.saturating_sub(old);
        let new = used.saturating_sub(real);
        t.nodes[s as usize].alloc = new;
        t.nodes[s as usize].size = new;
        t.nodes[0].alloc = real + new;
        t.nodes[0].size = t.nodes[0].size.saturating_sub(old) + new;
    }
}
