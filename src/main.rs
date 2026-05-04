// Phase 1.D.1 — capture GTK key events and surface the most recent
// keystroke in the grid.
//
// 1.C.3 painted the *welcome* buffer once at startup.  This phase adds
// shared mutable state (`Rc<RefCell<AppState>>') so the key controller
// can mutate the grid AFTER the initial paint and trigger a repaint
// via `queue_draw' — the substrate boilerplate Phase 1.D.2 will use
// to forward events into `emacs-command-loop' and re-mirror the
// updated buffer.
//
// Phase 1.D.2 (next): translate `gdk::Key' values into the symbolic
// representation `emacs-tui-event' / `emacs-command-loop' expect, push
// them via `(emacs-command-loop-feed-events ...)' / `(emacs-command-
// loop-step)', then re-query `(buffer-string)' and stamp the result
// into the buffer-mirror rows so live editing becomes visible.

mod grid;
mod nelisp_bridge;

use std::cell::RefCell;
use std::rc::Rc;

use grid::CharGrid;
use gtk::pango;
use gtk::pango::FontDescription;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, DrawingArea, EventControllerKey};
use nelisp_bridge::Session;

const APP_ID: &str = "org.nelisp.emacs.gtk";
const ROWS: usize = 24;
const COLS: usize = 80;
const FONT: &str = "Monospace 12";

/// Shared GUI state.  Mutated by the key controller, read by the draw
/// callback — both paths pass through `Rc<RefCell<_>>' so the
/// gtk4 single-threaded model keeps the borrows safe.
struct AppState {
    grid: CharGrid,
    last_key: String,
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

/// Measure (cell_w, cell_h, ascent) in pixels for the chosen monospace font.
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
            (insert "Welcome to nemacs-gtk\n")
            (insert "=====================\n")
            (insert "\n")
            (insert "This buffer was created in Layer-2 elisp via:\n")
            (insert "  (generate-new-buffer \"*welcome*\")\n")
            (insert "  (with-current-buffer ... (insert ...))\n")
            (insert "\n")
            (insert "Phase 1.D.1 — type any key; the row at the bottom\n")
            (insert "will mirror the keystroke captured by GTK.\n")
            (buffer-string))))"#
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

const STATUS_ROW: usize = ROWS - 2;

/// Stamp the "Last key" status line into the grid, clearing the row first
/// so a long previous label doesn't bleed past the current text.
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

fn build_initial_grid() -> CharGrid {
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
    g.put_str_centered(1, "Phase 1.D.1 — keyboard event capture");

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
        "(buffer-string)",
        if bootstrap_ok { "<see below>" } else { &buffer_result },
    );

    g.put_str(10, 2, "*welcome* buffer (mirrored from Layer 2):");
    for c in 2..COLS - 2 {
        g.put(11, c, '-');
    }
    let content_start_row = 12usize;
    let content_max_rows = (STATUS_ROW.saturating_sub(content_start_row)).saturating_sub(1);
    let content = if bootstrap_ok {
        unquote_printed(&buffer_result)
    } else {
        format!("[bootstrap failed]\n{buffer_result}")
    };
    for (i, line) in content.lines().take(content_max_rows).enumerate() {
        g.put_str(content_start_row + i, 4, line);
    }

    // Status separator + initial prompt line.
    for c in 2..COLS - 2 {
        g.put(STATUS_ROW - 1, c, '-');
    }
    put_status_line(&mut g, "");

    g
}

fn build_ui(app: &Application) {
    let (cell_w, cell_h, _ascent) = measure_cell();
    let canvas_w = (cell_w * COLS as f64).ceil() as i32;
    let canvas_h = (cell_h * ROWS as f64).ceil() as i32;

    let state = Rc::new(RefCell::new(AppState {
        grid: build_initial_grid(),
        last_key: String::new(),
    }));

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

    // Phase 1.D.1: capture every key press and surface its keysym name
    // + held modifiers in the status row.  No substrate dispatch yet —
    // 1.D.2 will translate `repr' to an `emacs-tui-event' representation
    // and call `(emacs-command-loop-feed-events ...)'.
    let key_controller = EventControllerKey::new();
    let state_for_key = state.clone();
    let area_for_key = area.clone();
    key_controller.connect_key_pressed(move |_, keyval, _keycode, modifier| {
        let name = keyval.name().map(|n| n.to_string()).unwrap_or_else(|| "?".into());
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
        st.last_key = repr.clone();
        put_status_line(&mut st.grid, &repr);
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
        // After the second put, only "Last key: a" should remain — the
        // long suffix from the first call must be cleared.
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
}
