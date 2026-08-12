# KernelOS v2

A fully-featured WebAssembly desktop environment built with Rust and Yew.

![KernelOS Screenshot](screenshot.png)

## Features

### 🖥️ Desktop Environment
- **Window Management**: Drag, resize, minimize, maximize windows
- **Taskbar**: Quick launch, running app indicators, system clock
- **Start Menu**: Searchable application launcher
- **Context Menus**: Right-click for quick actions
- **Desktop Icons**: Double-click to launch apps
- **Notifications**: Toast notifications for system events
- **Themes**: Dark/Light mode, customizable accent colors
- **Wallpapers**: Multiple gradient and solid color options

### 📱 Applications

| App | Description |
|-----|-------------|
| 📁 **File Explorer** | Browse, create, delete files and folders with list/grid views |
| 💻 **Terminal** | Full-featured terminal with 20+ commands (ls, cd, grep, find, etc.) |
| 🌐 **Browser** | Sandboxed iframe browser with history, a `vfs://` scheme, and Wikipedia search |
| 📝 **Text Editor** | Edit files with line numbers, word wrap, font sizing |
| 🔢 **Calculator** | Scientific calculator with memory functions |
| 🕐 **Clock** | Analog and digital clock with beautiful design |
| 🎨 **Paint** | Drawing app with brush, shapes, colors |
| 💣 **Minesweeper** | Classic minesweeper game |
| ⚙️ **Settings** | Customize themes, wallpapers, and system options |

Apps are declared once in [`src/apps.rs`](src/apps.rs). Adding one means adding a
registry entry and a component — the start menu, desktop icons, quick launch,
window sizing and window rendering all read from that list.

### 🌐 About the Browser

Pages load in a sandboxed iframe. A large part of the web refuses to be framed
via `X-Frame-Options` or CSP `frame-ancestors`, and there is no client-side way
around it — a proxy would mean a backend, which would cost the "runs entirely in
your browser" property.

| | |
|---|---|
| **Frames fine** | Wikipedia, Hacker News, rust-lang.org, doc.rust-lang.org, docs.rs, example.com |
| **Blocked** | Google, GitHub, DuckDuckGo, Bing |

A blocked frame is indistinguishable from a successful cross-origin one at the
JS level (both fire `load`), so the browser never guesses: every page keeps an
`↗` button that opens it in a real tab. Searches go to Wikipedia because the
major search engines all block framing.

`vfs:///home/documents` browses the virtual filesystem — directories list, files
render their contents.

### 💾 Virtual File System
- Persistent localStorage-backed file system
- Standard directory structure (/home, /applications, /system)
- Full CRUD operations on files and directories, including recursive move and copy
- Degrades to in-memory when local storage is unavailable (private browsing)

### 🔄 Session Persistence

Open windows, their positions and sizes, plus theme, accent and wallpaper are
restored on reload. Both are ordinary files inside the VFS, so they are
inspectable from inside the OS:

```
cat /system/config/theme.json
cat /system/config/session.json
```

## Quick Start

### Prerequisites
- [Rust](https://rustup.rs/) 1.92.0 (pinned by `rust-toolchain.toml`)
- [trunk](https://trunkrs.dev/) (`cargo install trunk`)
- wasm32 target (`rustup target add wasm32-unknown-unknown`)

### Build & Run

```bash
git clone https://github.com/Divhanthelion/kernelos.git
cd kernelos

# Install trunk if you haven't
cargo install trunk

# Add wasm target
rustup target add wasm32-unknown-unknown

# Build and stage every plugin with its 16 MiB hard memory cap
./build-plugins.sh

# Fetch the pinned Pyodide runtime (needed for the agent's run_python tool)
./fetch-pyodide.sh

# Run development server
trunk serve

# Or build for production
trunk build --release
```

Open http://localhost:8080 in your browser.

Run `./build-plugins.sh check` in CI or before committing to verify the staged
plugin manifests and WASM modules match a clean release build. The bundled
`hello` plugin installs automatically; use `pkg install doc-viewer` in the
KernelOS Terminal to exercise capability consent, persisted plugin state, and
read-only access to `/home/documents`.

## Project Structure

```
kernelosv2/
├── Cargo.toml           # Rust dependencies
├── index.html           # HTML entry point
├── styles.css           # Global styles
└── src/
    ├── lib.rs           # Main entry point
    ├── filesystem.rs    # Virtual file system
    └── components/
        ├── mod.rs       # Component exports
        ├── desktop.rs   # Main desktop component
        ├── window.rs    # Window management
        ├── taskbar.rs   # Taskbar with clock
        ├── start_menu.rs
        ├── context_menu.rs
        ├── notification.rs
        ├── terminal.rs  # Terminal emulator
        ├── file_explorer.rs
        ├── text_editor.rs
        ├── calculator.rs
        ├── clock.rs
        ├── paint.rs
        ├── minesweeper.rs
        └── settings.rs
```

## Terminal Commands

| Command | Description |
|---------|-------------|
| `ls [-l] [-a] [path]` | List directory contents |
| `cd [path]` | Change directory (`..`, `-` and `~` supported) |
| `pwd` | Print working directory |
| `cat [file]` | Display file contents |
| `mkdir [-p] [dir]` | Create directory |
| `touch [file]` | Create empty file |
| `rm [-r] [path]` | Remove file/directory |
| `mv [src] [dst]` | Move/rename |
| `cp [src] [dst]` | Copy file |
| `grep [pattern] [file]` | Search in file |
| `find [path] [name]` | Find files |
| `head/tail [file]` | Show first/last lines |
| `wc [file]` | Word/line count |
| `tree [path]` | Display directory tree |
| `whoami` | Display username |
| `date` | Display date/time |
| `clear` | Clear terminal |
| `history` | Command history |
| `help` | Show all commands |

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+S` | Save in text editor |
| `↑/↓` | Navigate terminal history |
| `Tab` | Auto-complete in terminal |
| `Ctrl+L` | Clear terminal |
| `Ctrl++/-` | Zoom text editor |

## Technologies

- **Rust** - Systems programming language
- **Yew** - React-like framework for Rust/WASM
- **WebAssembly** - Near-native performance in browser
- **LocalStorage** - Persistent file system storage
- **CSS3** - Modern styling with gradients and animations

## Browser Support

- Chrome 80+
- Firefox 75+
- Safari 14+
- Edge 80+

## License

MIT License - Feel free to use and modify!

## Contributing

Contributions welcome! Adding an app is a registry entry in
[`src/apps.rs`](src/apps.rs) plus a component. Some ideas:

- [x] Web browser iframe
- [ ] Image viewer with zoom/pan
- [ ] Music player
- [ ] More games (Snake, Tetris)
- [ ] Markdown preview (the browser's `vfs://` renderer is a good starting point)
- [ ] Code syntax highlighting
- [ ] File compression/decompression
- [ ] Multi-user support
- [ ] Cloud sync

### Theming

Theming is driven by a `data-theme` attribute on the document root plus the CSS
variables at the top of [`styles.css`](styles.css). Every component reads from
those variables, so light/dark and the accent colour apply everywhere —
including app interiors.

If you add a component, style it with the variables (`--window-bg`,
`--text-primary`, `--border-color`, `--accent-primary`, …) rather than literal
colours. Inline `style` attributes are reserved for genuinely dynamic values:
window geometry, the wallpaper gradient, colour swatches, editor font size.

---

Built with Rust and WebAssembly.

## License

MIT
