// Phase 1.C.3 — Layer 2 buffer state painted into the GTK grid.
//
// 1.C.2 demonstrated that a single Layer-2 module (`emacs-error') could
// be `(require)'-ed against a persistent NeLisp Session.  This phase
// scales that up:
//
//   - Run the smallest non-trivial slice of Layer 2 that yields a
//     working buffer abstraction (`generate-new-buffer' / `insert' /
//     `buffer-string').  The transitive require chain for
//     `emacs-buffer-builtins' is `cl-lib' → `nelisp-regex' →
//     `nelisp-text-buffer' → `nelisp-emacs-compat' →
//     `emacs-buffer-builtins'.
//
//   - On the elisp side, populate a `*welcome*' buffer with a multi-line
//     greeting then yield `(buffer-string)' back across the Rust boundary.
//
//   - On the Rust side, split the returned string on '\n' and stamp each
//     line into the central area of the `CharGrid', so the GTK window
//     literally mirrors the Layer-2 buffer content.
//
// Phase 1.D will replace the static greeting with a live event-driven
// redraw: keystrokes routed through `emacs-command-loop' mutate the
// buffer, the substrate calls back into a redraw bridge, and Pango
// repaints the affected cells.

mod grid;
mod nelisp_bridge;

use grid::CharGrid;
use gtk::pango;
use gtk::pango::FontDescription;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, DrawingArea};
use nelisp_bridge::Session;

const APP_ID: &str = "org.nelisp.emacs.gtk";
const ROWS: usize = 24;
const COLS: usize = 80;
const FONT: &str = "Monospace 12";

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

/// Truncate `s' to `max' characters with a one-char ellipsis suffix.
fn truncate_to(mut s: String, max: usize) -> String {
    if s.chars().count() > max {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        s = cut;
        s.push('…');
    }
    s
}

/// Render a probe row (label / form / result) into the grid.
fn put_probe_row(g: &mut CharGrid, row: usize, label: &str, form: &str, result: &str) {
    const LABEL_COL: usize = 2;
    const FORM_COL: usize = 22;
    const RESULT_COL: usize = 50;
    g.put_str(row, LABEL_COL, label);
    g.put_str(row, FORM_COL, form);
    g.put_str(row, RESULT_COL, &truncate_to(result.to_string(), COLS - RESULT_COL - 1));
}

/// Elisp form that sets up the *welcome* buffer in the Session and
/// returns its full text.  Runs once per process; after this the
/// buffer is reachable via `(get-buffer "*welcome*")'.
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
            (insert "The Rust GUI side then queried (buffer-string)\n")
            (insert "and painted these cells via Pango/Cairo.\n")
            (insert "\n")
            (insert "Phase 1.C.3 — Layer 2 buffer mirror.\n")
            (buffer-string))))"#
}

/// Strip outer quotes from a NeLisp printed string ("...") so we can
/// stamp the raw bytes into the grid.  Falls through unchanged for
/// non-quoted return values (e.g. error messages).
fn unquote_printed(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        // Decode the small subset of escapes the NeLisp printer emits:
        // \n \t \\ \"
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

fn build_welcome_grid() -> CharGrid {
    let mut g = CharGrid::blank(ROWS, COLS);
    let last_row = ROWS - 1;
    let last_col = COLS - 1;

    // Decorative border + corner markers.
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

    g.put_str_centered(1, "Phase 1.C.3 — Layer 2 buffer mirror");

    let mut session = Session::new();

    // ---- Section A: bootstrap ---------------------------------------------
    g.put_str(3, 2, "Bootstrap");
    put_probe_row(&mut g, 4, "step", "form", "=>");
    for c in 2..COLS - 2 {
        g.put(5, c, '-');
    }

    // 1) load-path priming + master substrate require (= emacs-init
    //    pulls every sibling module in the canonical order).
    let setup_result = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
    put_probe_row(&mut g, 6, "bootstrap", "(require 'emacs-init)", &setup_result);

    // 2) `emacs-buffer-builtins' is now already loaded by `emacs-init',
    //    so this require is just a `featurep' check.
    let req_result = session.eval_to_string("(require 'emacs-buffer-builtins)");
    put_probe_row(
        &mut g,
        7,
        "require buffer",
        "(require 'emacs-buffer-builtins)",
        &req_result,
    );

    // 3) build the welcome buffer + read its content back as a string
    let buffer_result = session.eval_to_string(welcome_buffer_form());

    let bootstrap_ok = !req_result.starts_with("ERR ") && !buffer_result.starts_with("ERR ");
    put_probe_row(
        &mut g,
        8,
        "buffer ready",
        "(buffer-string)",
        if bootstrap_ok { "<see below>" } else { &buffer_result },
    );

    // ---- Section B: rendered buffer --------------------------------------
    g.put_str(10, 2, "*welcome* buffer (mirrored from Layer 2):");
    for c in 2..COLS - 2 {
        g.put(11, c, '-');
    }

    // Buffer content rows 12..21 (10 rows of capacity).
    let content_start_row = 12usize;
    let content_max_rows = 10usize;
    let content = if bootstrap_ok {
        unquote_printed(&buffer_result)
    } else {
        format!("[bootstrap failed]\n{buffer_result}")
    };
    for (i, line) in content.lines().take(content_max_rows).enumerate() {
        g.put_str(content_start_row + i, 4, line);
    }

    g
}

fn build_ui(app: &Application) {
    let (cell_w, cell_h, _ascent) = measure_cell();
    let canvas_w = (cell_w * COLS as f64).ceil() as i32;
    let canvas_h = (cell_h * ROWS as f64).ceil() as i32;

    let g = build_welcome_grid();

    let area = DrawingArea::new();
    area.set_content_width(canvas_w);
    area.set_content_height(canvas_h);

    area.set_draw_func(move |_area, cr, _w, _h| {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();
        cr.set_source_rgb(0.0, 0.0, 0.0);

        let layout = pangocairo::functions::create_layout(cr);
        let desc = FontDescription::from_string(FONT);
        layout.set_font_description(Some(&desc));

        let mut buf = [0u8; 4];
        for row in 0..g.rows {
            for col in 0..g.cols {
                let ch = g.get(row, col);
                if ch == ' ' {
                    continue;
                }
                layout.set_text(ch.encode_utf8(&mut buf));
                cr.move_to(col as f64 * cell_w, row as f64 * cell_h);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
    });

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .build();
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
}
