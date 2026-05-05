// Phase 2.A — native menu bar.
//
// Adds a `gio::Menu' (= GMenuModel) with File / Edit / Help menus to
// the application.  Each item is wired to a `gio::SimpleAction':
//
//   File > Save     — placeholder (Phase 2.B file IO planned)
//   File > Quit     — closes the window
//   Edit > Cut      — placeholder (Phase 2.C clipboard planned)
//   Edit > Copy     — placeholder (Phase 2.C)
//   Edit > Paste    — placeholder (Phase 2.C)
//   Help > About    — echoes "nemacs-gtk Phase 2.A"
//
// Placeholder actions stamp a message into the echo area + queue a
// redraw (= same path the keyboard echo uses).  Quit calls
// `window.close()' which propagates to GTK's app-shutdown.  When
// later phases bring real `save-buffer' / `kill-region' / `yank'
// commands online, the placeholder closures are replaced with
// `command-loop-feed-events' calls so the menu dispatch routes
// through the same Layer 2 path as the keyboard.
//
// Phase 1.E — `(window-system)' / `(display-graphic-p)' return correct
// values for GUI dispatch.
//
// Earlier phases left the substrate display probes hard-coded to nil
// (= the no-op stubs in `emacs-stub.el').  Substrate Phase 1.E flips
// those stubs into a defvar-driven dispatch (`emacs-display-system')
// that this driver flips to `'gtk' before any code that branches on
// `(window-system)' / `(display-graphic-p)' runs.  The welcome buffer
// now displays both probe values inline so the wire-up is visible at
// runtime.
//
// Phase 1.D.4 — mode-line + echo-area layout.
//
// 1.D.3b moved key dispatch onto the substrate command-loop while the
// surface around the buffer was still scaffolding (= ASCII frame
// borders + bootstrap-probe rows + a "Last key:" status row).  This
// phase replaces that with the canonical Emacs three-region layout:
//
//   rows 0..MODE_LINE_ROW  → buffer area (= full canvas width)
//   row  MODE_LINE_ROW     → mode line  (highlighted, inverted text)
//   row  ECHO_AREA_ROW     → echo area  (= where messages land,
//                            currently shows the last key event)
//
// Mode line content is computed each refresh from `(buffer-name)' /
// `(line-number-at-pos)' / `(point)' / `(point-max)' / `major-mode'
// against the *welcome* buffer, padded with dashes to canvas width
// — the same shape `nemacs-main--initial-paint' renders on the TUI
// driver, just without the SGR sequences.  GTK provides the window
// title bar so we drop the top frame border that 1.D.3b carried over
// from the 1.A scaffolding.

mod grid;
mod nelisp_bridge;

use std::cell::RefCell;
use std::rc::Rc;

use grid::CharGrid;
use gtk::gdk;
use gtk::gio;
use gtk::pango;
use gtk::pango::FontDescription;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, DrawingArea, EventControllerKey};
use nelisp_bridge::Session;

const APP_ID: &str = "org.nelisp.emacs.gtk";
const ROWS: usize = 24;
const COLS: usize = 80;
const FONT: &str = "Monospace 12";

const MODE_LINE_ROW: usize = ROWS - 2;
const ECHO_AREA_ROW: usize = ROWS - 1;
const BUFFER_AREA_START: usize = 0;
const BUFFER_AREA_END: usize = MODE_LINE_ROW; // exclusive
const BUFFER_AREA_COL_START: usize = 0;
const BUFFER_AREA_COL_END: usize = COLS;

struct AppState {
    grid: CharGrid,
    session: Session,
    last_key: String,
    bootstrap_ok: bool,
    /// Screen-relative (row, col) of the buffer's `(point)` in the
    /// `*welcome*' buffer, recomputed after every key dispatch.  None
    /// when bootstrap failed or `(point)` lies outside the visible
    /// buffer-mirror window.
    cursor: Option<(usize, usize)>,
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn measure_cell() -> (f64, f64, f64) {
    let fontmap = pangocairo::FontMap::default();
    let ctx = fontmap.create_context();
    let desc = FontDescription::from_string(FONT);
    let metrics = ctx.metrics(Some(&desc), None);
    let scale = pango::SCALE as f64;
    let cell_w = metrics.approximate_digit_width() as f64 / scale;
    let ascent = metrics.ascent() as f64 / scale;
    let descent = metrics.descent() as f64 / scale;
    (cell_w, ascent + descent, ascent)
}

fn truncate_to(mut s: String, max: usize) -> String {
    if s.chars().count() > max {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        s = cut;
        s.push('…');
    }
    s
}

/// Compose a mode-line text matching the canonical Emacs shape:
///
///   `-U:--- *welcome*    L<N>    All    (Fundamental)` + dash pad
///
/// Buffer name is centred-ish (= follows the prefix); after the line
/// number / position / mode group the rest of the row is filled with
/// `-` to canvas width — same trailing-dash convention the TUI
/// backend prints.  All inputs default to `?` if the substrate query
/// returned an error.
fn format_mode_line(
    name: &str,
    line: &str,
    pos: &str,
    mode: &str,
    cols: usize,
) -> String {
    let body = format!(
        "-U:---  {name}    L{line}   {pos}   ({mode}) ",
        name = name,
        line = line,
        pos = pos,
        mode = mode,
    );
    let mut s = body;
    while s.chars().count() < cols {
        s.push('-');
    }
    if s.chars().count() > cols {
        s.chars().take(cols).collect()
    } else {
        s
    }
}

fn welcome_buffer_form() -> &'static str {
    // Probes are guarded with `fboundp' so a stale substrate (= a
    // canonical-clone fallback that hasn't picked up Phase 1.E yet)
    // doesn't take the welcome buffer down with a void-function error.
    // Same shape as `(if (fboundp 'foo) (foo) 'unbound)' that real
    // init.el uses for capability probes.
    r#"(progn
        (require 'emacs-buffer-builtins)
        (let ((buf (or (get-buffer "*welcome*")
                       (generate-new-buffer "*welcome*"))))
          (with-current-buffer buf
            (erase-buffer)
            (insert "Welcome to nemacs-gtk!\n")
            (insert "\n")
            (insert (format "Phase 1.E: (window-system) => %S\n"
                            (if (fboundp 'window-system)
                                (window-system) 'unbound)))
            (insert (format "          (display-graphic-p) => %S\n"
                            (if (fboundp 'display-graphic-p)
                                (display-graphic-p) 'unbound)))
            (insert "\n")
            (insert "Phase 2.A: native menu bar above (File / Edit / Help)\n")
            (insert "Type to insert; Backspace / Enter / arrows for\n")
            (insert "motion + edits.  Mode line refreshes after each key.\n")
            (insert "\n")
            (insert "> "))
          (buffer-name buf)))"#
}

fn unquote_printed(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        s.to_string()
    }
}

/// Stamp the echo area (= row `ECHO_AREA_ROW') with `text', clearing
/// the row first.  Empty text shows a placeholder hint.  Truncates
/// to canvas width with an ellipsis if needed.
fn put_echo_area(g: &mut CharGrid, text: &str) {
    for c in 0..COLS {
        g.put(ECHO_AREA_ROW, c, ' ');
    }
    let display = if text.is_empty() {
        "(press any key)".to_string()
    } else {
        format!("Last key: {text}")
    };
    g.put_str(ECHO_AREA_ROW, 0, &truncate_to(display, COLS));
}

/// Stamp the mode line (= row `MODE_LINE_ROW') with `text', padded /
/// truncated to canvas width.  Visual highlight (= inverted colours)
/// is the draw callback's responsibility, not ours.
fn put_mode_line(g: &mut CharGrid, text: &str) {
    for c in 0..COLS {
        g.put(MODE_LINE_ROW, c, ' ');
    }
    let chars: Vec<char> = text.chars().take(COLS).collect();
    for (i, ch) in chars.iter().enumerate() {
        g.put(MODE_LINE_ROW, i, *ch);
    }
}

/// Walk `buffer' counting chars/newlines to the 1-based POINT and
/// return the resulting (row, col) — both 0-based, relative to the
/// start of the buffer text (= NOT screen coordinates).
fn point_to_row_col(buffer: &str, point: usize) -> (usize, usize) {
    let target = point.saturating_sub(1);
    let mut row = 0usize;
    let mut col = 0usize;
    for (i, ch) in buffer.chars().enumerate() {
        if i == target {
            break;
        }
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (row, col)
}

/// Re-query the substrate for `*welcome*' state and re-stamp the
/// mode line on the grid.  Pulls `(buffer-name)' / `(line-number-
/// at-pos)' / `(point)' / `(point-max)' / `major-mode' (= names that
/// `emacs-init' bootstraps via `emacs-buffer-builtins' /
/// `emacs-line-builtins' / `emacs-mode-builtins').  Errors fall back
/// to "?" so the row never goes blank.
fn refresh_mode_line(grid: &mut CharGrid, session: &mut Session) {
    let probe = |form: &str, default: &str, session: &mut Session| -> String {
        let r = session.eval_to_string(form);
        if r.starts_with("ERR ") {
            default.into()
        } else {
            r
        }
    };
    let name_raw = probe(
        r#"(with-current-buffer (get-buffer "*welcome*") (buffer-name))"#,
        "?",
        session,
    );
    let name = unquote_printed(&name_raw);
    let line = probe(
        r#"(with-current-buffer (get-buffer "*welcome*") (line-number-at-pos))"#,
        "?",
        session,
    );
    let mode_raw = probe("(symbol-name major-mode)", "?", session);
    let mode = unquote_printed(&mode_raw);

    // Position indicator: "All" when the buffer fits in the visible
    // area, else a percentage.  Until we add scrolling we always show
    // the full buffer, so this is just "All".
    let pos = "All".to_string();

    let line_text = format_mode_line(&name, line.trim(), &pos, &mode, COLS);
    put_mode_line(grid, &line_text);
}

/// Re-query `(buffer-string)' + `(point)' for `*welcome*', re-stamp
/// the buffer text into the grid's editable region, and return the
/// screen-relative cursor position derived from the new point.
/// Called once at startup and after every key dispatch.
fn refresh_buffer_area(grid: &mut CharGrid, session: &mut Session) -> Option<(usize, usize)> {
    let text = session.eval_to_string(
        r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
    );
    let content = unquote_printed(&text);
    for row in BUFFER_AREA_START..BUFFER_AREA_END {
        for col in BUFFER_AREA_COL_START..BUFFER_AREA_COL_END {
            grid.put(row, col, ' ');
        }
    }
    let max_rows = BUFFER_AREA_END - BUFFER_AREA_START;
    let max_cols = BUFFER_AREA_COL_END - BUFFER_AREA_COL_START;
    for (i, line) in content.lines().take(max_rows).enumerate() {
        let truncated: String = line.chars().take(max_cols).collect();
        grid.put_str(BUFFER_AREA_START + i, BUFFER_AREA_COL_START, &truncated);
    }

    // Cursor: derive from elisp `(point)' against the same buffer text.
    let point_str = session
        .eval_to_string(r#"(with-current-buffer (get-buffer "*welcome*") (point))"#);
    let point: usize = point_str.trim().parse().ok()?;
    let (br, bc) = point_to_row_col(&content, point);
    if br < max_rows && bc <= max_cols {
        Some((BUFFER_AREA_START + br, BUFFER_AREA_COL_START + bc))
    } else {
        None
    }
}

/// Translate a GTK key event into the elisp event literal the
/// command-loop expects in the unread queue:
///
/// - integer literal "65" — printable ASCII, dispatched to `self-
///   insert-command' through the global keymap;
/// - quoted symbol "'backspace" — function keys / motion arrows,
///   matched against `(define-key m (vector 'backspace) ...)' bindings.
///
/// Returns `None` when the keysym has no handled mapping (= modifier
/// keys, function keys we haven't bound).  Caller skips the eval in
/// that case so the command-loop queue stays clean.
fn build_event_literal(
    keyval: gdk::Key,
    _modifier: gdk::ModifierType,
) -> Option<String> {
    let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
    match name.as_str() {
        "BackSpace" => Some("'backspace".into()),
        "Return" | "KP_Enter" => Some("'return".into()),
        "Left" => Some("'left".into()),
        "Right" => Some("'right".into()),
        "Up" => Some("'up".into()),
        "Down" => Some("'down".into()),
        _ => keyval
            .to_unicode()
            .filter(|c| !c.is_control())
            .map(|c| format!("{}", c as u32)),
    }
}

/// Run bootstrap + welcome buffer + keymap install, then populate
/// the buffer area + mode line for the first paint.  Returns the
/// fully-initialised state ready for the GTK event loop.
fn build_initial_state() -> AppState {
    let mut g = CharGrid::blank(ROWS, COLS);
    let mut session = Session::new();

    eprintln!(
        "[nemacs-gtk] layer2_src_path = {}",
        nelisp_bridge::layer2_src_path()
    );
    let setup_result = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
    eprintln!("[nemacs-gtk] layer2 setup = {setup_result}");
    // Phase 1.E — flip `emacs-display-system' BEFORE other bootstrap
    // forms so `(require 'emacs-buffer-builtins)' / future init.el
    // hooks see the GUI path on the first probe.
    let display_result =
        session.eval_to_string(nelisp_bridge::display_system_setup_form());
    eprintln!("[nemacs-gtk] display setup = {display_result}");
    eprintln!(
        "[nemacs-gtk] (fboundp 'window-system) = {}",
        session.eval_to_string("(fboundp 'window-system)")
    );
    let req_result = session.eval_to_string("(require 'emacs-buffer-builtins)");
    eprintln!("[nemacs-gtk] require buffer-builtins = {req_result}");
    let buffer_result = session.eval_to_string(welcome_buffer_form());
    eprintln!("[nemacs-gtk] welcome buffer = {buffer_result}");
    let keymap_result = session.eval_to_string(nelisp_bridge::command_loop_setup_form());
    eprintln!("[nemacs-gtk] keymap setup = {keymap_result}");
    let bootstrap_ok = !setup_result.starts_with("ERR ")
        && !display_result.starts_with("ERR ")
        && !req_result.starts_with("ERR ")
        && !buffer_result.starts_with("ERR ")
        && !keymap_result.starts_with("ERR ");

    let cursor = if bootstrap_ok {
        let c = refresh_buffer_area(&mut g, &mut session);
        refresh_mode_line(&mut g, &mut session);
        c
    } else {
        // Bootstrap diagnostic: stamp the failing step into the
        // top of the buffer area so the user sees what blew up.
        let diag = [
            ("layer2 setup", &setup_result),
            ("display-system 'gtk", &display_result),
            ("(require 'emacs-buffer-builtins)", &req_result),
            ("welcome buffer", &buffer_result),
            ("command-loop keymap", &keymap_result),
        ];
        let mut row = BUFFER_AREA_START;
        g.put_str(row, 0, "[bootstrap failed]");
        row += 2;
        for (label, result) in diag {
            if row >= BUFFER_AREA_END {
                break;
            }
            g.put_str(row, 0, &truncate_to(format!("{label}: {result}"), COLS));
            row += 1;
        }
        put_mode_line(&mut g, &format_mode_line("?", "?", "?", "?", COLS));
        None
    };

    put_echo_area(&mut g, "");

    AppState {
        grid: g,
        session,
        last_key: String::new(),
        bootstrap_ok,
        cursor,
    }
}

fn build_ui(app: &Application) {
    let (cell_w, cell_h, _ascent) = measure_cell();
    let canvas_w = (cell_w * COLS as f64).ceil() as i32;
    let canvas_h = (cell_h * ROWS as f64).ceil() as i32;

    let state = Rc::new(RefCell::new(build_initial_state()));

    let area = DrawingArea::new();
    area.set_content_width(canvas_w);
    area.set_content_height(canvas_h);

    let state_for_draw = state.clone();
    area.set_draw_func(move |_area, cr, _w, _h| {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();

        let layout = pangocairo::functions::create_layout(cr);
        let desc = FontDescription::from_string(FONT);
        layout.set_font_description(Some(&desc));

        let st = state_for_draw.borrow();
        let canvas_w = cell_w * COLS as f64;

        // Mode line background — paint a dark bar across row
        // MODE_LINE_ROW first so glyphs land on top in inverted colour.
        cr.set_source_rgb(0.18, 0.18, 0.22);
        cr.rectangle(0.0, MODE_LINE_ROW as f64 * cell_h, canvas_w, cell_h);
        let _ = cr.fill();

        // Phase 1.D.3a — block cursor at point.  Painted AFTER the
        // mode-line bar but BEFORE the buffer glyphs so the char in
        // the highlighted cell stays visible (= semi-transparent fill
        // darkens but doesn't occlude).
        if let Some((row, col)) = st.cursor {
            cr.set_source_rgba(0.2, 0.4, 0.9, 0.45);
            cr.rectangle(
                col as f64 * cell_w,
                row as f64 * cell_h,
                cell_w,
                cell_h,
            );
            let _ = cr.fill();
        }

        let mut buf = [0u8; 4];
        for row in 0..st.grid.rows {
            // Mode-line text uses inverted colour against the dark bar.
            if row == MODE_LINE_ROW {
                cr.set_source_rgb(0.94, 0.94, 0.94);
            } else {
                cr.set_source_rgb(0.0, 0.0, 0.0);
            }
            for col in 0..st.grid.cols {
                let ch = st.grid.get(row, col);
                if ch == ' ' {
                    continue;
                }
                layout.set_text(ch.encode_utf8(&mut buf));
                cr.move_to(col as f64 * cell_w, row as f64 * cell_h);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
    });

    let key_controller = EventControllerKey::new();
    let state_for_key = state.clone();
    let area_for_key = area.clone();
    key_controller.connect_key_pressed(move |_, keyval, _keycode, modifier| {
        let name = keyval
            .name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        let mods = if modifier.is_empty() {
            String::new()
        } else {
            format!(" mod={modifier:?}")
        };
        let unicode = keyval
            .to_unicode()
            .filter(|c| !c.is_control())
            .map(|c| format!(" '{c}'"))
            .unwrap_or_default();
        let repr = format!("{name}{mods}{unicode}");

        let mut st = state_for_key.borrow_mut();
        // Splay through one explicit deref so the borrow checker can
        // see `st.grid' and `st.session' as disjoint field accesses.
        let app: &mut AppState = &mut st;
        if app.bootstrap_ok {
            if let Some(literal) = build_event_literal(keyval, modifier) {
                let form =
                    nelisp_bridge::command_loop_dispatch_form("*welcome*", &literal);
                let _ = app.session.eval_to_string(&form);
                app.cursor = refresh_buffer_area(&mut app.grid, &mut app.session);
                refresh_mode_line(&mut app.grid, &mut app.session);
            }
        }
        app.last_key = repr.clone();
        put_echo_area(&mut app.grid, &repr);
        drop(st);
        area_for_key.queue_draw();
        glib::Propagation::Proceed
    });

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .show_menubar(true)
        .build();
    window.add_controller(key_controller);
    install_menu_bar(app, &window, state.clone(), area.clone());
    window.present();
}

/// Phase 2.A — build the application menu bar (= GMenuModel) +
/// register the SimpleActions that the items reference.  Uses
/// `app.<name>' action targets so a future per-window menu can
/// override individual entries via `win.<name>'.
fn install_menu_bar(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    area: DrawingArea,
) {
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    file.append(Some("Save"), Some("app.save"));
    file.append(Some("Quit"), Some("app.quit"));
    menu.append_submenu(Some("_File"), &file);

    let edit = gio::Menu::new();
    edit.append(Some("Cut"), Some("app.cut"));
    edit.append(Some("Copy"), Some("app.copy"));
    edit.append(Some("Paste"), Some("app.paste"));
    menu.append_submenu(Some("_Edit"), &edit);

    let help = gio::Menu::new();
    help.append(Some("About"), Some("app.about"));
    menu.append_submenu(Some("_Help"), &help);

    app.set_menubar(Some(&menu));

    // Helper closure factory: each placeholder action stamps `text'
    // into the echo area + queues a redraw.  Same shape the keyboard
    // handler uses, so the menu feels consistent with the keys.
    let make_placeholder = |text: &'static str| {
        let st = state.clone();
        let ar = area.clone();
        move |_a: &gio::SimpleAction, _: Option<&glib::Variant>| {
            let mut s = st.borrow_mut();
            put_echo_area(&mut s.grid, text);
            s.last_key = text.to_string();
            drop(s);
            ar.queue_draw();
        }
    };

    for (name, msg) in [
        ("save", "menu: Save (Phase 2.B planned)"),
        ("cut", "menu: Cut (Phase 2.C clipboard planned)"),
        ("copy", "menu: Copy (Phase 2.C clipboard planned)"),
        ("paste", "menu: Paste (Phase 2.C clipboard planned)"),
        ("about", "nemacs-gtk Phase 2.A — native menu bar"),
    ] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(make_placeholder(msg));
        app.add_action(&action);
    }

    // Quit closes the window (= triggers GTK's app-shutdown).
    let quit_action = gio::SimpleAction::new("quit", None);
    let win = window.clone();
    quit_action.connect_activate(move |_, _| win.close());
    app.add_action(&quit_action);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diagnostic — reproduces the user-reported "void-function:
    /// window-system" inside the welcome form.  Asserts the form evals
    /// cleanly so a regression here trips a test, not a runtime
    /// bootstrap-failed dialog.
    #[test]
    fn welcome_buffer_form_evals_cleanly_after_phase_1e_setup() {
        let mut s = Session::new();
        let setup = s.eval_to_string(&nelisp_bridge::layer2_setup_form());
        assert!(!setup.starts_with("ERR "), "layer2 setup failed: {setup}");
        let display =
            s.eval_to_string(nelisp_bridge::display_system_setup_form());
        assert!(!display.starts_with("ERR "), "display setup failed: {display}");
        let r = s.eval_to_string(welcome_buffer_form());
        assert!(
            !r.starts_with("ERR "),
            "welcome form errored: {r}"
        );
    }

    #[test]
    fn unquote_printed_strips_outer_quotes() {
        assert_eq!(unquote_printed("\"hello\""), "hello");
    }

    #[test]
    fn unquote_printed_decodes_newline_escape() {
        assert_eq!(unquote_printed("\"a\\nb\""), "a\nb");
    }

    #[test]
    fn unquote_printed_passes_through_unquoted() {
        assert_eq!(unquote_printed("ERR foo"), "ERR foo");
    }

    #[test]
    fn truncate_to_keeps_short_strings_intact() {
        assert_eq!(truncate_to("abc".to_string(), 10), "abc");
    }

    #[test]
    fn truncate_to_appends_ellipsis_on_overflow() {
        assert_eq!(truncate_to("abcdef".to_string(), 4), "abc…");
    }

    #[test]
    fn put_echo_area_clears_previous_text() {
        let mut g = CharGrid::blank(ROWS, COLS);
        put_echo_area(&mut g, "very-long-keysym-name");
        put_echo_area(&mut g, "a");
        let row: String = (0..COLS).map(|c| g.get(ECHO_AREA_ROW, c)).collect();
        assert!(row.contains("Last key: a"));
        assert!(!row.contains("very-long"));
    }

    #[test]
    fn put_echo_area_initial_empty_shows_prompt() {
        let mut g = CharGrid::blank(ROWS, COLS);
        put_echo_area(&mut g, "");
        let row: String = (0..COLS).map(|c| g.get(ECHO_AREA_ROW, c)).collect();
        assert!(row.contains("(press any key)"));
    }

    #[test]
    fn format_mode_line_pads_with_dashes_and_truncates() {
        let line = format_mode_line("*welcome*", "1", "All", "Fundamental", 80);
        assert_eq!(line.chars().count(), 80);
        assert!(line.contains("*welcome*"));
        assert!(line.contains("L1"));
        assert!(line.contains("All"));
        assert!(line.contains("(Fundamental)"));
        assert!(line.ends_with('-'));

        let narrow = format_mode_line("xxxxxxxxxxxxxxxxxxxx", "1", "All", "Fund", 10);
        assert_eq!(narrow.chars().count(), 10);
    }

    #[test]
    fn put_mode_line_overwrites_full_row() {
        let mut g = CharGrid::blank(ROWS, COLS);
        put_mode_line(&mut g, "AAAA");
        let r: String = (0..COLS).map(|c| g.get(MODE_LINE_ROW, c)).collect();
        assert!(r.starts_with("AAAA"));
        // Cells past the input remain blank (= we cleared the row).
        assert_eq!(g.get(MODE_LINE_ROW, 5), ' ');
        // Re-stamp shorter content does not leak old chars.
        put_mode_line(&mut g, "B");
        let r2: String = (0..COLS).map(|c| g.get(MODE_LINE_ROW, c)).collect();
        assert!(r2.starts_with("B"));
        assert!(!r2.contains("AAAA"));
    }

    #[test]
    fn refresh_mode_line_writes_buffer_name() {
        let mut s = Session::new();
        let setup = s.eval_to_string(&nelisp_bridge::layer2_setup_form());
        assert!(!setup.starts_with("ERR "), "setup failed: {setup}");
        // Phase 1.E — welcome_buffer_form embeds `(window-system)' so
        // we must flip the display system before populating the
        // buffer; otherwise the format probe fires against the
        // nil-default state and the row content is irrelevant.
        let _ = s.eval_to_string(nelisp_bridge::display_system_setup_form());
        let _ = s.eval_to_string(welcome_buffer_form());
        let mut g = CharGrid::blank(ROWS, COLS);
        refresh_mode_line(&mut g, &mut s);
        let row: String = (0..COLS).map(|c| g.get(MODE_LINE_ROW, c)).collect();
        assert!(row.contains("*welcome*"), "row = {row:?}");
        // Line number depends on how many newlines welcome_buffer_form
        // wrote — assert just the prefix shape.
        assert!(row.contains("L"), "row = {row:?}");
        assert!(row.contains("(fundamental-mode)"), "row = {row:?}");
    }

    #[test]
    fn build_event_literal_maps_named_keys() {
        let none = gdk::ModifierType::empty();
        // Letter 'A' → integer literal "65".
        let a = gdk::Key::from_name("A").expect("'A' keysym");
        assert_eq!(build_event_literal(a, none), Some("65".into()));

        // Multi-char keysyms → quoted symbol literals.  Each lookup
        // returns Some(Key) on every gtk4 build that includes the
        // standard X11 keysym table.
        for (name, expected) in [
            ("BackSpace", "'backspace"),
            ("Return", "'return"),
            ("Left", "'left"),
            ("Right", "'right"),
            ("Up", "'up"),
            ("Down", "'down"),
        ] {
            let k = gdk::Key::from_name(name).expect(name);
            assert_eq!(
                build_event_literal(k, none),
                Some(expected.into()),
                "keysym {name}"
            );
        }
    }

    #[test]
    fn build_event_literal_skips_control_chars() {
        // Control_L is a modifier-only key (no unicode, name = "Control_L").
        let ctrl = gdk::Key::from_name("Control_L").expect("Control_L");
        assert_eq!(build_event_literal(ctrl, gdk::ModifierType::empty()), None);
    }

    #[test]
    fn point_to_row_col_handles_simple_buffer() {
        // "abc\nxy" with point=1 should give (0, 0).
        assert_eq!(point_to_row_col("abc\nxy", 1), (0, 0));
        // point=4 (= the '\n') should still report end of first line.
        assert_eq!(point_to_row_col("abc\nxy", 4), (0, 3));
        // point=5 (= 'x') should give (1, 0).
        assert_eq!(point_to_row_col("abc\nxy", 5), (1, 0));
        // point=7 (= one past 'y') should give (1, 2).
        assert_eq!(point_to_row_col("abc\nxy", 7), (1, 2));
    }

    #[test]
    fn dispatch_form_self_insert_round_trip() {
        let mut session = Session::new();
        let setup = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
        assert!(!setup.starts_with("ERR "));
        let _ = session.eval_to_string(welcome_buffer_form());

        // Simulate `(insert "X")' against *welcome* through the same
        // form shape `build_dispatch_form' produces.
        let r = session.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (insert "X"))"#,
        );
        assert!(!r.starts_with("ERR "), "insert failed: {r}");
        let buf = session.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        assert!(buf.contains('X'), "expected X in buffer; got {buf}");
    }

    #[test]
    fn dispatch_form_backspace_round_trip() {
        let mut session = Session::new();
        let _ = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
        let _ = session.eval_to_string(welcome_buffer_form());
        // Insert two chars then backspace once.
        let _ = session.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (insert "AB"))"#,
        );
        let _ = session.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (when (> (point) 1) (delete-backward-char 1)))"#,
        );
        let buf = session.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        // Buffer should contain A but not the trailing B (deleted).
        // Note: "AB" had B last; delete-backward-char removed B.
        assert!(buf.contains("A"));
        // Check the last 4 chars don't contain "AB" any more.
        assert!(!buf.contains("AB"), "backspace did not remove B: {buf}");
    }

    #[test]
    fn refresh_buffer_area_renders_buffer_string() {
        let mut session = Session::new();
        let setup = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
        assert!(!setup.starts_with("ERR "), "setup failed: {setup}");
        let prepare = session.eval_to_string(
            r#"(progn
                (require 'emacs-buffer-builtins)
                (let ((buf (or (get-buffer "*welcome*")
                               (generate-new-buffer "*welcome*"))))
                  (with-current-buffer buf
                    (erase-buffer)
                    (insert "abc\nxyz\n")))
                t)"#,
        );
        assert!(!prepare.starts_with("ERR "), "prepare failed: {prepare}");
        let mut g = CharGrid::blank(ROWS, COLS);
        refresh_buffer_area(&mut g, &mut session);
        // Row 12 should contain "abc"; row 13 should contain "xyz".
        let r12: String = (0..COLS).map(|c| g.get(BUFFER_AREA_START, c)).collect();
        let r13: String = (0..COLS).map(|c| g.get(BUFFER_AREA_START + 1, c)).collect();
        assert!(r12.contains("abc"), "row 12 = {r12:?}");
        assert!(r13.contains("xyz"), "row 13 = {r13:?}");
    }
}
