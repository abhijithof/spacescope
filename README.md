# SpaceScope

A fast, free disk-space analyzer for Windows. See what's eating your drive, learn what each folder is and whether it's safe to delete, and clean it up in one click.

## Download

Grab **`SpaceScope_x.y.z_x64-setup.exe`** from the [latest release](../../releases/latest) and run it. It installs per-user (no admin needed).

> Windows SmartScreen may warn because the installer isn't code-signed. Click **More info → Run anyway**.

## Features

- **Fast full-drive scan** — reads directories with `NtQueryDirectoryFile`, counts hidden/system files, de-duplicates hard links (e.g. WinSxS), skips junctions. A ~1.2M-file C: drive scans in about 30 seconds.
- **Treemap view** of every folder and file.
- **"Can I delete it?" knowledge base** — explains what common folders are (caches, build output, installers, app data) and marks them safe, cache, or keep.
- **One-click cleanup** of safe items, with guards against deleting through junctions or locked/read-only files.
- **Low impact** — optional low-priority (background I/O) scan mode. Never auto-scans on open; it loads your last saved results.
- **Ask AI (optional)** — explains unknown folders using a local [Ollama](https://ollama.com) model (`qwen3:8b`). Nothing is sent to the cloud.

## Build from source

Requirements: [Node.js](https://nodejs.org) 20+, [Rust](https://rustup.rs) (stable, MSVC), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Windows.

```bash
npm install
npx tauri dev      # run in development
npx tauri build    # installer → src-tauri/target/release/bundle/nsis/
```

Extra checks:

```bash
cd src-tauri
cargo run --release --example bench -- C:   # scan + print cleanup report
cargo run --release --example selftest      # delete-safety tests
```

## Adding knowledge

Folder rules live in `src-tauri/kb/rules.json` (path patterns; the deepest/most specific match wins) and file-extension info in `src-tauri/kb/exts.json`. PRs adding rules are welcome.

## Tech

Tauri v2 · Rust · React + TypeScript · d3-hierarchy

## License

[MIT](LICENSE)
