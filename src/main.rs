// Phase 1.C.1 — embed NeLisp runtime + render eval results in the grid.
//
// Builds on Phase 1.B's Pango monospace grid by replacing the static
// diagonal-and-dots pattern with a precomputed `CharGrid' that holds
// the textual readout of a few NeLisp eval probes — proving the
// embedded interpreter is alive and reachable from the GTK main thread.
//
// Phase 1.C.2 will add load-path + `(require '...)' so we can pull in
// Layer 2 elisp from `nelisp-emacs/src/'.  Phase 1.C.3 will replace the
// startup-only fill with a redraw triggered after each command-loop
// step, mirroring how the TUI driver repaints the terminal grid.

mod grid;
mod nelisp_bridge;

use grid::CharGrid;
use gtk::pango;
use gtk::pango::FontDescription;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, DrawingArea};

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

/// Build the static welcome grid for Phase 1.C.1 — header, NeLisp probe
/// results, footer.  Visible proof that `nelisp::eval::eval_str' is wired
/// up and produces correct values from inside the GTK process.
fn build_welcome_grid() -> CharGrid {
    let mut g = CharGrid::blank(ROWS, COLS);

    // Border decoration (corner markers + thin top/bottom lines).
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

    // Headers
    g.put_str_centered(0, " nemacs-gtk ");
    g.put_str_centered(2, "Phase 1.C.1 — embedded NeLisp runtime sanity check");

    // Probe lines.  Each row shows form + result (or ERR ...).
    let probes: &[(&str, &str)] = &[
        ("integer arithmetic", "(+ 1 2)"),
        ("multiplication",     "(* 7 8)"),
        ("string concat",      "(concat \"hello, \" \"world\")"),
        ("list length",        "(length '(a b c d e))"),
        ("nested call",        "(+ (* 3 4) (* 5 6))"),
        ("cons + car/cdr",     "(car (cdr '(a b c)))"),
    ];
    let label_col = 2usize;
    let form_col = 22usize;
    let result_col = 50usize;
    g.put_str(4, label_col, "label");
    g.put_str(4, form_col, "form");
    g.put_str(4, result_col, "=>");
    for c in 0..(COLS - 4) {
        g.put(5, c + 2, '-');
    }
    for (i, (label, form)) in probes.iter().enumerate() {
        let row = 6 + i;
        g.put_str(row, label_col, label);
        g.put_str(row, form_col, form);
        let mut result = nelisp_bridge::eval_to_string(form);
        // Truncate so the result column never overflows the grid.
        let max_result_len = COLS - result_col - 1;
        if result.chars().count() > max_result_len {
            result.truncate(max_result_len);
            result.push('…');
        }
        g.put_str(row, result_col, &result);
    }

    // Footer
    g.put_str_centered(
        last_row - 2,
        "All probes evaluated by the embedded NeLisp runtime",
    );
    g.put_str_centered(last_row, " close X to quit ");

    g
}

fn build_ui(app: &Application) {
    let (cell_w, cell_h, ascent) = measure_cell();
    let canvas_w = (cell_w * COLS as f64).ceil() as i32;
    let canvas_h = (cell_h * ROWS as f64).ceil() as i32;

    // Compute the grid once at startup.  Future phases will refresh on
    // every command-loop step and queue_draw().
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
        let _ = ascent; // reserved for baseline-precise rendering in 1.C.3
    });

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .build();
    window.present();
}
