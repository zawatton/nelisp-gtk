# nelisp-gtk

> ⚠️ **Status: early / exploratory research project.**
> This repository is **not** a ready-to-use Emacs replacement or a stable
> GUI toolkit.  It is an in-progress substrate that boots a GTK4 window,
> embeds a NeLisp Lisp runtime, and exposes a small set of `nelisp-gtk-*`
> builtins to the elisp side.  Expect breaking changes on every commit
> and most "real Emacs" features to be **missing** for the foreseeable
> future.  See the [Phase plan](#phase-plan) for honest scope.

## What is this?

`nelisp-gtk` is the Layer 3 GTK4 GUI backend in the
[NeLisp](https://github.com/zawatton/nelisp) /
[nelisp-emacs](https://github.com/zawatton/nelisp-emacs) stack.  The
Rust binary owns the GTK4 main loop, font shaping (Pango), and 2D
drawing (Cairo); it embeds a NeLisp `Session` and hands every visible
behaviour — buffer layout, mode-line composition, key dispatch, redraw
policy — back to elisp through a thin `nelisp-gtk-*` builtin surface.

The repository was previously named `nelisp-emacs-gtk` and built as
the binary `nemacs-gtk`.  It was renamed to `nelisp-gtk` (binary:
`nelisp-gtk`) on 2026-05-14 to reflect a broader long-term scope:

- **Primary current use case:** an Emacs-shaped frontend on top of
  `nelisp-emacs` (the Layer 2 substrate that ports Emacs C → elisp).
  All Phase 1.x / Phase 2.x work below is in service of this.
- **Future scope:** non-Emacs nelisp / elisp applications.  The
  `nelisp-gtk-*` Rust→elisp builtin surface is intentionally generic
  (grid put / draw / key poll / iterate / quit), so a third-party
  elisp app can build on the same backend without going through
  `emacs-frame.el` / `emacs-command-loop.el`.

## Position in the layered architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Layer 3  (THIS REPO)                                        │
│  nelisp-gtk     — Rust binary, GTK4 / Pango / Cairo /        │
│                   GLib main loop, embeds a NeLisp Session    │
└─────────────▲────────────────────────────────────────────────┘
              │  nelisp-gtk-* builtins (grid put / draw /
              │  key poll / iterate / quit)
              │
┌─────────────┴────────────────────────────────────────────────┐
│  Layer 2  (upstream: nelisp-emacs)                           │
│  emacs-frame.el · emacs-redisplay.el · emacs-tui-event.el ·  │
│  emacs-command-loop.el · …                                   │
└─────────────▲────────────────────────────────────────────────┘
              │
┌─────────────┴────────────────────────────────────────────────┐
│  Layer 1  (upstream: nelisp)                                 │
│  NeLisp core Lisp runtime (read / eval / GC / FFI)           │
└──────────────────────────────────────────────────────────────┘
```

The TUI sibling at Layer 3 is
`nelisp-emacs/src/emacs-tui-backend.el` + `emacs-tui-terminfo.el`.
This repo is the GUI sibling — same Layer 3 interface
(`nelisp-display-*`), different backend.

## Tech stack (locked 2026-05-04)

- **GTK4** ≥ 4.10 via `gtk4-rs` (`gtk` crate v0.9, `package = "gtk4"`)
- **Pango** for font shaping + text layout
- **Cairo** for vector drawing
- **GLib** event loop owns the main thread

System dependencies (Debian/Ubuntu):

```sh
sudo apt install libgtk-4-dev libpango1.0-dev libcairo2-dev
```

## Build / Run

```sh
cargo build --release
./target/release/nelisp-gtk      # release
cargo run                        # debug
```

The binary expects two upstream checkouts on disk:

- `nelisp` (the Rust Lisp runtime) reachable as a Cargo path
  dependency.  Resolved transitively via
  `nelisp-emacs/vendor/nelisp` by the default `Cargo.toml`.
- `nelisp-emacs` (the elisp substrate) reachable at one of
  the candidate paths probed by `nelisp_bridge::layer2_src_path()`,
  or via the `NEMACS_HOME` / `NEMACS_LAYER2_SRC` environment
  variables.

Phase 1.C will lock the exact integration (workspace member? git
submodule? cargo path dep?) — at present it is filesystem
convention plus env overrides.

## Phase plan

The Emacs-frontend use case is decomposed into the phases below.
Anything past Phase 2.F is research-grade — do not budget against
it.

| Phase | Scope                                                   | Close gate                                                        |
|-------|---------------------------------------------------------|-------------------------------------------------------------------|
| 1.A   | GTK4 scaffolding + hello window                          | `cargo run` opens window with placeholder Pango text              |
| 1.B   | Pango monospace char grid                                | 24×80 grid renders fixed test string at correct cell positions    |
| 1.C   | NeLisp embed + Layer 2 elisp load                        | `bin/nelisp-gtk` evals `nemacs-loadup`, paints welcome buffer     |
| 1.D   | Keyboard event integration                               | `self-insert` / motion / Backspace via `emacs-command-loop`       |
| 1.E   | `(window-system)` + `display-graphic-p` correct          | GUI-vs-TUI dispatch works in `init.el`                            |
| 2.A   | Native menu bar (`GMenuModel`)                           | File / Edit menus visible                                         |
| 2.B   | Native file dialog                                       | `C-x C-f` opens GTK file chooser                                  |
| 2.C   | Clipboard (X selection compat)                           | `x-select-text` / `x-get-selection` round-trip                    |
| 2.D   | Mouse events                                             | `mouse-1/2/3` + wheel feed into command loop                      |
| 2.E   | Resize + multi-frame                                     | `(make-frame)` opens a 2nd OS window                              |
| 2.F   | Proportional font + face merge                           | variable-width faces in buffer                                    |

## Using `nelisp-gtk` for non-Emacs apps

This is **aspirational** at the time of writing — no non-Emacs
frontend has been built yet — but the builtin surface is shaped to
permit it:

```elisp
;; Equivalent of nelisp-gtk-frontend.el's hand-off, minus all the
;; emacs-* substrate calls:
(nelisp-gtk-init-window :rows 24 :cols 80 :title "my-app")
(nelisp-gtk-put-cell 0 0 ?H 'default)
(nelisp-gtk-put-cell 0 1 ?i 'default)
(nelisp-gtk-paint-frame)
(nelisp-gtk-iterate :until-quit t)
```

Concretely this means: if you want to write a GTK app whose UI logic
lives in elisp running on NeLisp (no Emacs buffer model, no
`emacs-command-loop.el`), the builtins are designed to make that
possible without forking the backend.  Pull requests that
sharpen the boundary are welcome.

## Repo conventions

- `Cargo.lock` is committed (binary crate, reproducible builds).
- Source of truth for visual behaviour is **elisp**, not Rust.
- Rust changes that bake policy into the backend (instead of
  exposing a builtin and letting elisp decide) are rejected on
  review.

## License

GPL-3.0-or-later (declared in `Cargo.toml`).  A `LICENSE` file will be
added before any release tagging.

## Related repositories

- [`nelisp`](https://github.com/zawatton/nelisp) — Layer 1, NeLisp core runtime
- [`nelisp-emacs`](https://github.com/zawatton/nelisp-emacs) — Layer 2, Emacs C → elisp port + TUI backend

## Acknowledgements

This is a personal research project by zawatton, developed
collaboratively with Claude (Anthropic).  No funding, no roadmap
commitment, no SLA — interest, issues, and PRs are welcome but please
calibrate expectations against the Phase plan above.
