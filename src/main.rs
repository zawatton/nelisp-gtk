// Phase 1.D.2 — interactive buffer editing.
//
// 1.D.1 captured GTK key events but only echoed them in a status row;
// the welcome-buffer area stayed static.  This phase wires those events
// into the embedded NeLisp Session so each keystroke mutates the live
// `*welcome*' buffer (= self-insert / Backspace / Enter / Left / Right)
// and the resulting `(buffer-string)` is re-queried + re-stamped into
// the grid before `queue_draw` repaints.
//
// We bypass the full `emacs-command-loop' dispatcher this round and
// translate keys directly to the corresponding edit primitive.  Phase
// 1.D.3 will route through the command loop + show a visible cursor.

mod grid;
mod nelisp_bridge;

use std::cell::RefCell;
use std::rc::Rc;

use grid::CharGrid;
use gtk::gdk;
use gtk::pango;
use gtk::pango::FontDescription;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, DrawingArea, EventControllerKey};
use nelisp_bridge::Session;

const APP_ID: &str = "org.nelisp.emacs.gtk";
const ROWS: usize = 24;
const COLS: usize = 80;
const FONT: &str = "Monospace 12";

const STATUS_ROW: usize = ROWS - 2;
const BUFFER_AREA_START: usize = 12;
const BUFFER_AREA_END: usize = STATUS_ROW - 1; // exclusive
const BUFFER_AREA_COL_START: usize = 4;
const BUFFER_AREA_COL_END: usize = COLS - 2; // exclusive

struct AppState {
    grid: CharGrid,
    session: Session,
    last_key: String,
    bootstrap_ok: bool,
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

fn put_probe_row(g: &mut CharGrid, row: usize, label: &str, form: &str, result: &str) {
    const LABEL_COL: usize = 2;
    const FORM_COL: usize = 22;
    const RESULT_COL: usize = 50;
    g.put_str(row, LABEL_COL, label);
    g.put_str(row, FORM_COL, form);
    g.put_str(row, RESULT_COL, &truncate_to(result.to_string(), COLS - RESULT_COL - 1));
}

fn welcome_buffer_form() -> &'static str {
    r#"(progn
        (require 'emacs-buffer-builtins)
        (let ((buf (or (get-buffer "*welcome*")
                       (generate-new-buffer "*welcome*"))))
          (with-current-buffer buf
            (erase-buffer)
            (insert "Welcome to nemacs-gtk!\n")
            (insert "\n")
            (insert "Phase 1.D.2: type printable keys to insert text,\n")
            (insert "press Backspace to delete, Enter for newline,\n")
            (insert "Left/Right to move point.  Edits round-trip\n")
            (insert "through the embedded NeLisp Session.\n")
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

/// Stamp the "Last key:" status line, clearing the row first.
fn put_status_line(g: &mut CharGrid, last_key: &str) {
    for c in 2..(COLS - 2) {
        g.put(STATUS_ROW, c, ' ');
    }
    let text = if last_key.is_empty() {
        "(press any key)".to_string()
    } else {
        format!("Last key: {last_key}")
    };
    g.put_str(STATUS_ROW, 2, &truncate_to(text, COLS - 4));
}

/// Re-query `(buffer-string)` for `*welcome*' and re-stamp the result
/// into the grid's editable buffer area.  Called once at startup and
/// after every key dispatch.
fn refresh_buffer_area(grid: &mut CharGrid, session: &mut Session) {
    let text = session.eval_to_string(r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#);
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
}

/// Translate a GTK key event into an elisp form that mutates the
/// `*welcome*' buffer.  Returns an empty string when the key has no
/// handled binding (= the caller skips the eval).
fn build_dispatch_form(keyval: gdk::Key, _modifier: gdk::ModifierType) -> String {
    let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
    match name.as_str() {
        "BackSpace" => {
            r#"(with-current-buffer (get-buffer "*welcome*") (when (> (point) 1) (delete-backward-char 1)))"#
                .to_string()
        }
        "Return" => r#"(with-current-buffer (get-buffer "*welcome*") (newline))"#.to_string(),
        "Left" => {
            r#"(with-current-buffer (get-buffer "*welcome*") (when (> (point) 1) (backward-char 1)))"#
                .to_string()
        }
        "Right" => {
            r#"(with-current-buffer (get-buffer "*welcome*") (when (< (point) (point-max)) (forward-char 1)))"#
                .to_string()
        }
        _ => {
            if let Some(ch) = keyval.to_unicode().filter(|c| !c.is_control()) {
                let escaped = match ch {
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    _ => ch.to_string(),
                };
                format!(r#"(with-current-buffer (get-buffer "*welcome*") (insert "{}"))"#, escaped)
            } else {
                String::new()
            }
        }
    }
}

/// Build the static frame, run bootstrap + welcome buffer setup, and
/// populate the editable buffer area for the first time.  Returns the
/// fully-initialised state ready for the GTK event loop.
fn build_initial_state() -> AppState {
    let mut g = CharGrid::blank(ROWS, COLS);
    let last_row = ROWS - 1;
    let last_col = COLS - 1;

    g.put(0, 0, '+');
    g.put(0, last_col, '+');
    g.put(last_row, 0, '+');
    g.put(last_row, last_col, '+');
    for c in 1..last_col {
        g.put(0, c, '-');
        g.put(last_row, c, '-');
    }
    g.put_str_centered(0, " nemacs-gtk ");
    g.put_str_centered(last_row, " close X to quit ");
    g.put_str_centered(1, "Phase 1.D.2 — interactive buffer editing");

    let mut session = Session::new();

    g.put_str(3, 2, "Bootstrap");
    put_probe_row(&mut g, 4, "step", "form", "=>");
    for c in 2..COLS - 2 {
        g.put(5, c, '-');
    }
    let setup_result = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
    put_probe_row(&mut g, 6, "bootstrap", "(require 'emacs-init)", &setup_result);
    let req_result = session.eval_to_string("(require 'emacs-buffer-builtins)");
    put_probe_row(
        &mut g,
        7,
        "require buffer",
        "(require 'emacs-buffer-builtins)",
        &req_result,
    );
    let buffer_result = session.eval_to_string(welcome_buffer_form());
    let bootstrap_ok = !req_result.starts_with("ERR ") && !buffer_result.starts_with("ERR ");
    put_probe_row(
        &mut g,
        8,
        "buffer ready",
        "(buffer-name buf)",
        if bootstrap_ok { &buffer_result } else { &buffer_result },
    );

    g.put_str(10, 2, "*welcome* buffer (live, editable):");
    for c in 2..COLS - 2 {
        g.put(11, c, '-');
    }

    if bootstrap_ok {
        refresh_buffer_area(&mut g, &mut session);
    } else {
        let msg = format!("[bootstrap failed]\n{buffer_result}");
        for (i, line) in msg.lines().take(BUFFER_AREA_END - BUFFER_AREA_START).enumerate() {
            g.put_str(BUFFER_AREA_START + i, BUFFER_AREA_COL_START, line);
        }
    }

    for c in 2..COLS - 2 {
        g.put(STATUS_ROW - 1, c, '-');
    }
    put_status_line(&mut g, "");

    AppState {
        grid: g,
        session,
        last_key: String::new(),
        bootstrap_ok,
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
        cr.set_source_rgb(0.0, 0.0, 0.0);

        let layout = pangocairo::functions::create_layout(cr);
        let desc = FontDescription::from_string(FONT);
        layout.set_font_description(Some(&desc));

        let st = state_for_draw.borrow();
        let mut buf = [0u8; 4];
        for row in 0..st.grid.rows {
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
            let form = build_dispatch_form(keyval, modifier);
            if !form.is_empty() {
                let _ = app.session.eval_to_string(&form);
                refresh_buffer_area(&mut app.grid, &mut app.session);
            }
        }
        app.last_key = repr.clone();
        put_status_line(&mut app.grid, &repr);
        drop(st);
        area_for_key.queue_draw();
        glib::Propagation::Proceed
    });

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .build();
    window.add_controller(key_controller);
    window.present();
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn put_status_line_clears_previous_text() {
        let mut g = CharGrid::blank(ROWS, COLS);
        put_status_line(&mut g, "very-long-keysym-name");
        put_status_line(&mut g, "a");
        let row: String = (0..COLS).map(|c| g.get(STATUS_ROW, c)).collect();
        assert!(row.contains("Last key: a"));
        assert!(!row.contains("very-long"));
    }

    #[test]
    fn put_status_line_initial_empty_shows_prompt() {
        let mut g = CharGrid::blank(ROWS, COLS);
        put_status_line(&mut g, "");
        let row: String = (0..COLS).map(|c| g.get(STATUS_ROW, c)).collect();
        assert!(row.contains("(press any key)"));
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
