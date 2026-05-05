// Phase 2 architecture pivot — GTK4 backend exposed as elisp builtins.
//
// All GUI state lives in `GtkState' wrapped in `Rc<RefCell<...>>'.  GTK
// callbacks (= key controller `connect_key_pressed', drawing-area
// `set_draw_func') and elisp-side primitives (registered via
// `Env::register_extern_builtin') share the same Rc handle, so each
// closure borrows briefly + releases — no static globals, no thread
// safety needed (= GTK is single-threaded).
//
// The exposed builtin surface is intentionally minimal — GTK plumbing
// only.  Every layout / mode-line / dispatch decision lives in
// `nemacs-gtk-frontend.el' on the substrate side.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::glib::translate::IntoGlib;
use gtk::pango::{self, FontDescription};
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea, EventControllerKey};

use nelisp::eval::{Env, EvalError};
use nelisp::reader::Sexp;

use crate::grid::CharGrid;

const FONT: &str = "Monospace 12";
const APP_ID: &str = "org.nelisp.emacs.gtk";

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub keysym: u32,
    pub mods: u32,
    pub unicode: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum MouseKind {
    Press,
    Release,
    ScrollUp,
    ScrollDown,
}

#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    pub kind: MouseKind,
    /// GDK button number (= 1/2/3 = left/middle/right).  Carried for
    /// press / release; meaningless for scroll (set to 0).
    pub button: u32,
    pub row: usize,
    pub col: usize,
    pub mods: u32,
}

pub struct GtkState {
    pub initialized: bool,
    pub app: Option<Application>,
    pub window: Option<ApplicationWindow>,
    pub area: Option<DrawingArea>,
    pub grid: CharGrid,
    pub cell_w: f64,
    pub cell_h: f64,
    pub cursor: Option<(usize, usize)>,
    pub mode_line_row: Option<usize>,
    pub key_queue: VecDeque<KeyEvent>,
    pub menu_event_queue: VecDeque<String>,
    pub mouse_event_queue: VecDeque<MouseEvent>,
    pub quit: bool,
}

impl GtkState {
    pub fn new() -> Self {
        Self {
            initialized: false,
            app: None,
            window: None,
            area: None,
            grid: CharGrid::blank(0, 0),
            cell_w: 0.0,
            cell_h: 0.0,
            cursor: None,
            mode_line_row: None,
            key_queue: VecDeque::new(),
            menu_event_queue: VecDeque::new(),
            mouse_event_queue: VecDeque::new(),
            quit: false,
        }
    }
}

/// Pango/Cairo cell-size probe.  Run once at GTK init; the cell grid
/// uses the resulting (width, height) to stage glyph positions.
fn measure_cell() -> (f64, f64) {
    let fontmap = pangocairo::FontMap::default();
    let ctx = fontmap.create_context();
    let desc = FontDescription::from_string(FONT);
    let metrics = ctx.metrics(Some(&desc), None);
    let scale = pango::SCALE as f64;
    let cell_w = metrics.approximate_digit_width() as f64 / scale;
    let ascent = metrics.ascent() as f64 / scale;
    let descent = metrics.descent() as f64 / scale;
    (cell_w, ascent + descent)
}

fn want_int(args: &[Sexp], idx: usize, name: &str) -> Result<i64, EvalError> {
    match args.get(idx) {
        Some(Sexp::Int(n)) => Ok(*n),
        Some(other) => Err(EvalError::WrongType {
            expected: format!("{name}: integer arg #{idx}"),
            got: other.clone(),
        }),
        None => Err(EvalError::ArithError(format!(
            "{name}: missing arg #{idx}"
        ))),
    }
}

fn want_string(args: &[Sexp], idx: usize, name: &str) -> Result<String, EvalError> {
    match args.get(idx) {
        Some(s) if s.is_string() => Ok(s.as_string_owned().unwrap_or_default()),
        Some(other) => Err(EvalError::WrongType {
            expected: format!("{name}: string arg #{idx}"),
            got: other.clone(),
        }),
        None => Err(EvalError::ArithError(format!(
            "{name}: missing arg #{idx}"
        ))),
    }
}

/// Convert a key event to the elisp tuple `(KEYSYM MODS UNICODE)` —
/// integers all the way so the frontend can `caar`/`cadr`/`caddr` cheaply.
fn key_event_to_sexp(ev: KeyEvent) -> Sexp {
    Sexp::list_from(&[
        Sexp::Int(ev.keysym as i64),
        Sexp::Int(ev.mods as i64),
        Sexp::Int(ev.unicode as i64),
    ])
}

/// Push a press/release event onto the mouse queue.  Converts the
/// raw pixel coords (= what GTK hands the gesture callback) into
/// 0-based cell `(row, col)' against the current cell metrics.
/// Coords past the canvas edge clamp to the last valid cell.
fn push_mouse(
    state: &Rc<RefCell<GtkState>>,
    kind: MouseKind,
    button: u32,
    x: f64,
    y: f64,
) {
    let mut g = state.borrow_mut();
    if g.cell_w <= 0.0 || g.cell_h <= 0.0 {
        return; // pre-init paint pass — drop event
    }
    let col = (x / g.cell_w).floor().max(0.0) as usize;
    let row = (y / g.cell_h).floor().max(0.0) as usize;
    let col = col.min(g.grid.cols.saturating_sub(1));
    let row = row.min(g.grid.rows.saturating_sub(1));
    g.mouse_event_queue.push_back(MouseEvent {
        kind,
        button,
        row,
        col,
        mods: 0,
    });
}

/// Convert a mouse event to the elisp tuple `(KIND BUTTON ROW COL MODS)`.
/// KIND is a quoted symbol ('press / 'release / 'scroll-up / 'scroll-down)
/// so the frontend can `eq'-dispatch.
fn mouse_event_to_sexp(ev: MouseEvent) -> Sexp {
    let kind = match ev.kind {
        MouseKind::Press => "press",
        MouseKind::Release => "release",
        MouseKind::ScrollUp => "scroll-up",
        MouseKind::ScrollDown => "scroll-down",
    };
    Sexp::list_from(&[
        Sexp::Symbol(kind.into()),
        Sexp::Int(ev.button as i64),
        Sexp::Int(ev.row as i64),
        Sexp::Int(ev.col as i64),
        Sexp::Int(ev.mods as i64),
    ])
}

/// Register every `nelisp-gtk-*' builtin against `env'.  The closures
/// each clone the `state' Rc so they own a reference for their
/// lifetime (= the Env's).
pub fn register_all(env: &mut Env, state: Rc<RefCell<GtkState>>) {
    // ----- nelisp-gtk-init (rows cols) -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-init", move |args, _env| {
            let rows = want_int(args, 0, "nelisp-gtk-init")? as usize;
            let cols = want_int(args, 1, "nelisp-gtk-init")? as usize;
            let already = st.borrow().initialized;
            if already {
                return Ok(Sexp::T);
            }
            init_gtk(&st, rows, cols)?;
            Ok(Sexp::T)
        });
    }

    // ----- nelisp-gtk-grid-put-row (row str) -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-grid-put-row", move |args, _env| {
            let row = want_int(args, 0, "nelisp-gtk-grid-put-row")? as usize;
            let s = want_string(args, 1, "nelisp-gtk-grid-put-row")?;
            let mut g = st.borrow_mut();
            // Clear the row first so a shorter string doesn't leak the
            // previous row's tail.
            for c in 0..g.grid.cols {
                g.grid.put(row, c, ' ');
            }
            g.grid.put_str(row, 0, &s);
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-grid-clear () -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-grid-clear", move |_args, _env| {
            let mut g = st.borrow_mut();
            let rows = g.grid.rows;
            let cols = g.grid.cols;
            for r in 0..rows {
                for c in 0..cols {
                    g.grid.put(r, c, ' ');
                }
            }
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-set-cursor (row col)  or  (nil)  -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-set-cursor", move |args, _env| {
            let mut g = st.borrow_mut();
            match (args.get(0), args.get(1)) {
                (Some(Sexp::Int(r)), Some(Sexp::Int(c))) => {
                    g.cursor = Some((*r as usize, *c as usize));
                }
                _ => {
                    g.cursor = None;
                }
            }
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-set-mode-line-row (row | nil) -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-set-mode-line-row", move |args, _env| {
            let mut g = st.borrow_mut();
            g.mode_line_row = match args.get(0) {
                Some(Sexp::Int(r)) => Some(*r as usize),
                _ => None,
            };
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-redraw () -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-redraw", move |_args, _env| {
            let g = st.borrow();
            if let Some(area) = &g.area {
                area.queue_draw();
            }
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-poll-key () -> (KEYSYM MODS UNICODE) | nil -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-poll-key", move |_args, _env| {
            let mut g = st.borrow_mut();
            match g.key_queue.pop_front() {
                Some(ev) => Ok(key_event_to_sexp(ev)),
                None => Ok(Sexp::Nil),
            }
        });
    }

    // ----- nelisp-gtk-iterate (blocking) -----
    {
        env.register_extern_builtin("nelisp-gtk-iterate", move |args, _env| {
            let blocking = !matches!(args.get(0), Some(Sexp::Nil) | None);
            let ctx = glib::MainContext::default();
            // One iteration; may_block = the arg.  Returns whether work
            // happened — we ignore (caller polls the key queue + redraws
            // unconditionally).
            ctx.iteration(blocking);
            Ok(Sexp::Nil)
        });
    }

    // ----- nelisp-gtk-should-quit () -----
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-should-quit", move |_args, _env| {
            let g = st.borrow();
            Ok(if g.quit { Sexp::T } else { Sexp::Nil })
        });
    }

    // ----- nelisp-gtk-set-menu-bar SPEC -----
    //
    // SPEC shape (= elisp data, walked recursively):
    //
    //   ((LABEL  ENTRY  ENTRY ...)        ; submenu
    //    (LABEL . ACTION-NAME-STRING))    ; leaf
    //
    // Both LABELs and ACTION-NAME-STRINGs are elisp strings.  When a
    // leaf is clicked the ACTION-NAME-STRING is pushed onto the
    // `menu_event_queue' so elisp can `(nelisp-gtk-poll-menu-event)'
    // and dispatch.  Calling this builtin a second time replaces the
    // previous menu model.
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-set-menu-bar", move |args, _env| {
            let spec = args.get(0).cloned().unwrap_or(Sexp::Nil);
            install_menu_bar(&st, &spec)
        });
    }

    // ----- nelisp-gtk-poll-menu-event () -> STRING | nil -----
    {
        let st = state.clone();
        env.register_extern_builtin(
            "nelisp-gtk-poll-menu-event",
            move |_args, _env| {
                let mut g = st.borrow_mut();
                match g.menu_event_queue.pop_front() {
                    Some(s) => Ok(Sexp::Str(s)),
                    None => Ok(Sexp::Nil),
                }
            },
        );
    }

    // ----- nelisp-gtk-poll-mouse () -> (KIND BUTTON ROW COL MODS) | nil -----
    //
    // KIND symbols: 'press / 'release / 'scroll-up / 'scroll-down.
    // ROW / COL are 0-based cell coords (= pixel coords ÷ cell metrics).
    // BUTTON is the GDK button number for press/release, 0 for scroll.
    {
        let st = state.clone();
        env.register_extern_builtin(
            "nelisp-gtk-poll-mouse",
            move |_args, _env| {
                let mut g = st.borrow_mut();
                match g.mouse_event_queue.pop_front() {
                    Some(ev) => Ok(mouse_event_to_sexp(ev)),
                    None => Ok(Sexp::Nil),
                }
            },
        );
    }

    // ----- nelisp-gtk-clipboard-set TEXT -----
    //
    // Push TEXT (= elisp string) onto the GDK display's primary clipboard.
    // No-op + returns nil for empty TEXT.  Returns t on success.
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-clipboard-set", move |args, _env| {
            let text = want_string(args, 0, "nelisp-gtk-clipboard-set")?;
            if text.is_empty() {
                return Ok(Sexp::Nil);
            }
            let clip = clipboard_for(&st)?;
            clip.set_text(&text);
            Ok(Sexp::T)
        });
    }

    // ----- nelisp-gtk-clipboard-get () -> STRING | nil -----
    //
    // Synchronously fetch the current clipboard text via GDK's async
    // `read_text_async' API by spinning the GLib MainContext until the
    // callback fires (or a 500 ms timeout — clipboard reads can hang
    // when the source app is unresponsive; we'd rather return nil than
    // freeze the UI).
    {
        let st = state.clone();
        env.register_extern_builtin("nelisp-gtk-clipboard-get", move |_args, _env| {
            let clip = clipboard_for(&st)?;
            Ok(read_clipboard_text_sync(&clip)
                .map(Sexp::Str)
                .unwrap_or(Sexp::Nil))
        });
    }
}

/// Resolve the GDK clipboard for the current display.  Prefers the
/// app's own ApplicationWindow display when available; falls back to
/// the default GDK display otherwise (= covers boot-time queries
/// before the window is built, though that path shouldn't normally
/// fire because callers gate on `(nelisp-gtk-init)' having run).
fn clipboard_for(
    state: &Rc<RefCell<GtkState>>,
) -> Result<gdk::Clipboard, EvalError> {
    let g = state.borrow();
    if !g.initialized {
        return Err(EvalError::Internal(
            "nelisp-gtk-clipboard-*: window not initialised — \
             call `(nelisp-gtk-init ROWS COLS)' first"
                .into(),
        ));
    }
    let display = if let Some(w) = g.window.as_ref() {
        WidgetExt::display(w)
    } else {
        match gdk::Display::default() {
            Some(d) => d,
            None => {
                return Err(EvalError::Internal(
                    "nelisp-gtk-clipboard-*: no GDK display available".into(),
                ));
            }
        }
    };
    Ok(display.clipboard())
}

/// Synchronously read text off `clipboard'.  Bridges GTK4's async-only
/// API by:
///   1. Kick off `read_text_async' with a callback that drops the
///      result into a shared cell.
///   2. Schedule a 500 ms timeout that flips a "give up" flag.
///   3. Spin `MainContext::iteration(true)' until either the cell or
///      the flag fills — `iteration(true)' blocks until at least one
///      source dispatches, so this is a busy-wait without burning CPU.
///
/// Single-threaded — borrows on the shared `Rc<RefCell<...>>' never
/// overlap because the iteration call dispatches the callback inline.
fn read_clipboard_text_sync(clipboard: &gdk::Clipboard) -> Option<String> {
    let result: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
    let result_cb = result.clone();
    clipboard.read_text_async(
        None::<&gtk::gio::Cancellable>,
        move |res| {
            let s = match res {
                Ok(Some(g)) => Some(g.to_string()),
                _ => None,
            };
            *result_cb.borrow_mut() = Some(s);
        },
    );

    let timed_out = Rc::new(RefCell::new(false));
    let timed_out_cb = timed_out.clone();
    glib::timeout_add_local_once(
        std::time::Duration::from_millis(500),
        move || {
            *timed_out_cb.borrow_mut() = true;
        },
    );

    let ctx = glib::MainContext::default();
    loop {
        if result.borrow().is_some() {
            break;
        }
        if *timed_out.borrow() {
            break;
        }
        ctx.iteration(true);
    }
    let out = result.borrow().clone().flatten();
    out
}

/// Walk a `Sexp' menu SPEC and install it as the application's
/// menubar.  See the comment on the `nelisp-gtk-set-menu-bar' builtin
/// for the SPEC shape.  Errors out if no Application + Window are up
/// yet (= caller forgot to `(nelisp-gtk-init ...)' first).
fn install_menu_bar(
    state: &Rc<RefCell<GtkState>>,
    spec: &Sexp,
) -> Result<Sexp, EvalError> {
    // Safety: we read app + window once, drop the borrow before
    // mutating GTK state — `set_menubar' / `add_action' don't re-enter
    // our state, so no double-borrow risk.
    let (app, window) = {
        let g = state.borrow();
        match (g.app.clone(), g.window.clone()) {
            (Some(a), Some(w)) => (a, w),
            _ => {
                return Err(EvalError::Internal(
                    "nelisp-gtk-set-menu-bar: window not initialised — \
                     call `(nelisp-gtk-init ROWS COLS)' first"
                        .into(),
                ));
            }
        }
    };

    // Drop pre-existing menu actions so re-running this builtin yields
    // a clean menubar.  We use a fixed prefix `menu-' for our actions
    // so a partial wipe is safe.
    for action_name in app.list_actions() {
        let s: String = action_name.into();
        if s.starts_with("menu-") {
            app.remove_action(&s);
        }
    }

    let root = gio::Menu::new();
    build_menu_recursive(&root, spec, &app, state);
    app.set_menubar(Some(&root));
    window.set_show_menubar(true);
    Ok(Sexp::T)
}

/// Recursively populate `parent' from `entries' (= a Sexp list whose
/// each element is a menu entry).  See `install_menu_bar' for shape.
fn build_menu_recursive(
    parent: &gio::Menu,
    entries: &Sexp,
    app: &Application,
    state: &Rc<RefCell<GtkState>>,
) {
    for entry in sexp_list_iter(entries) {
        // Each entry must be a cons cell (LABEL . REST).
        let (label, rest) = match &entry {
            Sexp::Cons(h, t) => {
                let head = h.borrow().clone();
                let tail = t.borrow().clone();
                let label = match head.as_string_owned() {
                    Some(s) => s,
                    None => continue,
                };
                (label, tail)
            }
            _ => continue,
        };
        match rest {
            // Leaf: cdr is a string action name.
            ref s if s.is_string() => {
                let action_name = s.as_string_owned().unwrap_or_default();
                install_leaf_action(parent, app, state, &label, &action_name);
            }
            // Submenu: cdr is a list of more entries.
            ref s if matches!(s, Sexp::Cons(_, _) | Sexp::Nil) => {
                let sub = gio::Menu::new();
                build_menu_recursive(&sub, s, app, state);
                parent.append_submenu(Some(&label), &sub);
            }
            _ => continue,
        }
    }
}

fn install_leaf_action(
    menu: &gio::Menu,
    app: &Application,
    state: &Rc<RefCell<GtkState>>,
    label: &str,
    action_name: &str,
) {
    // GAction names live in the app's action group; reference them
    // from the menu model via "app.<name>".  Prefix everything with
    // `menu-' so `nelisp-gtk-set-menu-bar' can wipe its slice cleanly
    // on re-install (= avoid colliding with any future built-in
    // app actions).
    let gaction_name = format!("menu-{action_name}");
    let action_target = format!("app.{gaction_name}");
    menu.append(Some(label), Some(&action_target));

    let action = gio::SimpleAction::new(&gaction_name, None);
    let st = state.clone();
    let action_name_owned = action_name.to_string();
    action.connect_activate(move |_, _| {
        st.borrow_mut()
            .menu_event_queue
            .push_back(action_name_owned.clone());
    });
    app.add_action(&action);
}

/// Iterate over a Sexp proper list (= Cons chain terminated by Nil).
/// Stops at the first non-Cons cdr (= dotted pair / improper list).
fn sexp_list_iter(s: &Sexp) -> Vec<Sexp> {
    let mut out = Vec::new();
    let mut cur = s.clone();
    loop {
        match cur {
            Sexp::Cons(h, t) => {
                out.push(h.borrow().clone());
                cur = t.borrow().clone();
            }
            _ => break,
        }
    }
    out
}

/// Build the GTK Application + Window + DrawingArea + key controller.
/// Wires the GTK signal callbacks against the same `state' Rc that the
/// elisp builtins use.
fn init_gtk(
    state: &Rc<RefCell<GtkState>>,
    rows: usize,
    cols: usize,
) -> Result<(), EvalError> {
    {
        let mut g = state.borrow_mut();
        g.grid = CharGrid::blank(rows, cols);
    }
    let app = Application::builder().application_id(APP_ID).build();

    // The activate handler builds the actual window + widgets.  We
    // capture `state' so the handler can populate it before returning.
    let state_for_activate = state.clone();
    app.connect_activate(move |app| {
        build_window(app, &state_for_activate, rows, cols);
    });

    // Drive the activate handler synchronously by registering the app
    // and dispatching the `activate' action; we cannot use `app.run()'
    // because that would block on the GLib main loop forever (= elisp
    // wants step-by-step iteration via `nelisp-gtk-iterate').
    app.register(None::<&gio::Cancellable>)
        .map_err(|e| EvalError::Internal(format!("app.register: {e}")))?;
    app.activate();

    {
        let mut g = state.borrow_mut();
        g.app = Some(app);
        let (cell_w, cell_h) = measure_cell();
        g.cell_w = cell_w;
        g.cell_h = cell_h;
        g.initialized = true;
    }
    Ok(())
}

fn build_window(
    app: &Application,
    state: &Rc<RefCell<GtkState>>,
    rows: usize,
    cols: usize,
) {
    let (cell_w, cell_h) = measure_cell();
    let canvas_w = (cell_w * cols as f64).ceil() as i32;
    let canvas_h = (cell_h * rows as f64).ceil() as i32;

    let area = DrawingArea::new();
    area.set_content_width(canvas_w);
    area.set_content_height(canvas_h);

    // ----- Draw callback — paints the current grid + cursor + mode-line.
    let st_for_draw = state.clone();
    area.set_draw_func(move |_a, cr, _w, _h| {
        let g = st_for_draw.borrow();
        if g.grid.rows == 0 || g.grid.cols == 0 {
            return;
        }
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();

        let layout = pangocairo::functions::create_layout(cr);
        let desc = FontDescription::from_string(FONT);
        layout.set_font_description(Some(&desc));

        let canvas_w = g.cell_w * g.grid.cols as f64;

        // Mode line bar.
        if let Some(ml_row) = g.mode_line_row {
            cr.set_source_rgb(0.18, 0.18, 0.22);
            cr.rectangle(0.0, ml_row as f64 * g.cell_h, canvas_w, g.cell_h);
            let _ = cr.fill();
        }

        // Block cursor.
        if let Some((row, col)) = g.cursor {
            cr.set_source_rgba(0.2, 0.4, 0.9, 0.45);
            cr.rectangle(
                col as f64 * g.cell_w,
                row as f64 * g.cell_h,
                g.cell_w,
                g.cell_h,
            );
            let _ = cr.fill();
        }

        let mut buf = [0u8; 4];
        for row in 0..g.grid.rows {
            if Some(row) == g.mode_line_row {
                cr.set_source_rgb(0.94, 0.94, 0.94);
            } else {
                cr.set_source_rgb(0.0, 0.0, 0.0);
            }
            for col in 0..g.grid.cols {
                let ch = g.grid.get(row, col);
                if ch == ' ' {
                    continue;
                }
                layout.set_text(ch.encode_utf8(&mut buf));
                cr.move_to(col as f64 * g.cell_w, row as f64 * g.cell_h);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
    });

    // ----- Key controller — pushes events onto the queue.
    let key_controller = EventControllerKey::new();
    let st_for_key = state.clone();
    key_controller.connect_key_pressed(move |_, keyval, _keycode, modifier| {
        let ev = KeyEvent {
            keysym: keyval.into_glib(),
            mods: modifier.bits(),
            unicode: keyval.to_unicode().map(|c| c as u32).unwrap_or(0),
        };
        st_for_key.borrow_mut().key_queue.push_back(ev);
        glib::Propagation::Proceed
    });

    // ----- Mouse click controller — press + release on any button.
    // GestureClick with `set_button(0)' captures every button so we
    // can distinguish 1/2/3 (= left/middle/right) on the elisp side.
    let click = gtk::GestureClick::new();
    click.set_button(0);
    let st_for_press = state.clone();
    click.connect_pressed(move |gesture, _n_press, x, y| {
        push_mouse(&st_for_press, MouseKind::Press, gesture.current_button(), x, y);
    });
    let st_for_release = state.clone();
    click.connect_released(move |gesture, _n_press, x, y| {
        push_mouse(
            &st_for_release,
            MouseKind::Release,
            gesture.current_button(),
            x,
            y,
        );
    });
    area.add_controller(click);

    // ----- Scroll wheel controller — direction-only for MVP.
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let st_for_scroll = state.clone();
    scroll.connect_scroll(move |_c, _dx, dy| {
        let kind = if dy < 0.0 {
            MouseKind::ScrollUp
        } else if dy > 0.0 {
            MouseKind::ScrollDown
        } else {
            return glib::Propagation::Proceed;
        };
        let ev = MouseEvent { kind, button: 0, row: 0, col: 0, mods: 0 };
        st_for_scroll.borrow_mut().mouse_event_queue.push_back(ev);
        glib::Propagation::Proceed
    });
    area.add_controller(scroll);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .build();
    window.add_controller(key_controller);

    // Close-request → flag quit + stop the close (= elisp loop drains
    // and tears down on the next iteration).
    let st_for_close = state.clone();
    window.connect_close_request(move |_| {
        st_for_close.borrow_mut().quit = true;
        glib::Propagation::Proceed
    });

    window.present();

    let mut g = state.borrow_mut();
    g.window = Some(window);
    g.area = Some(area);
}

// `gio` is re-exported by gtk4 but we need `Cancellable` directly.
use gtk::gio;
