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
| 📝 **Text Editor** | Edit files with line numbers, word wrap, font sizing |
| 🔢 **Calculator** | Scientific calculator with memory functions |
| 🕐 **Clock** | Analog and digital clock with beautiful design |
| 🎨 **Paint** | Drawing app with brush, shapes, colors |
| 💣 **Minesweeper** | Classic minesweeper game |
| ⚙️ **Settings** | Customize themes, wallpapers, and system options |

### 💾 Virtual File System
- Persistent localStorage-backed file system
- Standard directory structure (/home, /applications, /system)
- Full CRUD operations on files and directories

## Quick Start

### Prerequisites
- [Rust](https://rustup.rs/) (1.70+)
- [trunk](https://trunkrs.dev/) (`cargo install trunk`)
- wasm32 target (`rustup target add wasm32-unknown-unknown`)

### Build & Run

```bash
# Clone or download the project
cd kernelosv2

# Install trunk if you haven't
cargo install trunk

# Add wasm target
rustup target add wasm32-unknown-unknown

# Run development server
trunk serve

# Or build for production
trunk build --release
```

Open http://localhost:8080 in your browser.

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
| `cd [path]` | Change directory |
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

Contributions welcome! Some ideas for expansion:
- [ ] Image viewer with zoom/pan
- [ ] Music player
- [ ] Web browser iframe
- [ ] More games (Snake, Tetris)
- [ ] Markdown preview
- [ ] Code syntax highlighting
- [ ] File compression/decompression
- [ ] Multi-user support
- [ ] Cloud sync

---

Built with ❤️ using Rust and WebAssembly
