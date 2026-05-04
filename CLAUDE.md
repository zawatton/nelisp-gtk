# nelisp-emacs-gtk — Linux GTK4 GUI display backend

## Position in the layered architecture

- **Layer 1**: NeLisp Rust runtime (= upstream, repo `nelisp` / `nelisp-emacs/vendor/nelisp`)
- **Layer 2**: nelisp-emacs substrate elisp (= upstream, repo `nelisp-emacs`)
  — `emacs-frame.el`, `emacs-redisplay.el`, `emacs-tui-event.el`, `emacs-command-loop.el`, …
- **Layer 3**: GUI display backend (= **THIS REPO**, target Phase 11.C in Doc 43)
  — provides `nemacs-gtk` standalone binary, plugs into the
  `nelisp-display-*` interface that `emacs-frame.el` already exposes.

The TUI backend at Layer 3 is in `nelisp-emacs/src/emacs-tui-backend.el`
+ `emacs-tui-terminfo.el`. This repo is the GUI sibling.

## Tech stack (locked 2026-05-04)

- **GTK4** (>= 4.10) via gtk4-rs (`gtk` crate v0.9, `package = "gtk4"`)
- **Pango** for font shaping + text layout
- **Cairo** for vector drawing (where Pango doesn't suffice)
- **GLib** event loop owns the main thread

System dependencies (Debian/Ubuntu):
```
sudo apt install libgtk-4-dev libpango1.0-dev libcairo2-dev
```

## Phase plan

| Phase | Scope | Close gate |
|-------|-------|-----------|
| 1.A   | GTK4 scaffolding + hello window | `cargo run` opens window with placeholder Pango text |
| 1.B   | Pango monospace char grid       | 24x80 grid renders fixed test string at correct cell positions |
| 1.C   | NeLisp embed + Layer 2 elisp load | bin/nemacs-gtk evals nemacs-loadup, paints welcome buffer |
| 1.D   | Keyboard event integration      | self-insert / motion / Backspace working via emacs-command-loop |
| 1.E   | `(window-system)` + `display-graphic-p` returning correct values | GUI-vs-TUI dispatch works in init.el |
| 2.A   | Native menu bar (GMenuModel)    | File / Edit menus visible |
| 2.B   | Native file dialog              | C-x C-f opens GTK file chooser |
| 2.C   | Clipboard (X selection compat)  | x-select-text / x-get-selection round-trip |
| 2.D   | Mouse events                    | mouse-1/2/3 + wheel-up/down feed into command loop |
| 2.E   | Resize + multi-frame            | (make-frame) opens 2nd OS window |
| 2.F   | Proportional font + face merge  | variable-width faces in buffer |

## Repo conventions

- **Cargo.lock is committed** (binary crate, reproducible builds).
- Build: `cargo build --release` → `target/release/nemacs-gtk`.
- Run: `cargo run` (debug) or `./target/release/nemacs-gtk` (release).
- Layer 2 elisp is currently NOT vendored (Phase 1.A scope = Rust only).
  Phase 1.C will vendor or path-reference `nelisp-emacs/src/`.

## Upstream dependency ordering

This repo expects:
- `nelisp-emacs` checked out at a sibling path or vendored under `vendor/nelisp-emacs/`.
- `nelisp` (Rust runtime) reachable via the `nelisp-emacs/vendor/nelisp` chain.

Phase 1.C spec will lock the exact integration: workspace member?
git submodule? cargo path dep? — TBD.
