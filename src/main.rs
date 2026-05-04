// Phase 1.C.2 — embedded NeLisp + Layer 2 elisp `(require ...)' chain.
//
// Phase 1.C.1 proved the embedded interpreter could evaluate plain
// elisp arithmetic / list ops in a fresh global env.  This phase adds:
//
//   - a long-lived `Session' (= persistent Env) so successive evals
//     share state, mirroring the canonical `bin/nemacs' boot flow
//   - `load-path' priming pointing at `nelisp-emacs/src/' (Layer 2)
//   - `(require 'emacs-error)' as the smallest sanity-check Layer-2
//     module, with follow-up probes confirming the polyfilled
//     `user-error' / `display-warning' / `define-error' symbols are
//     reachable in the same session afterwards
//
// Phase 1.C.3 will replace this static welcome paint with a redraw
// triggered after each command-loop step (= a logical-buffer mirror
// driven by the embedded runtime).

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
        // Char-safe truncate: keep `max - 1' chars then append ellipsis.
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
    g.put_str_centered(2, "Phase 1.C.2 — embedded NeLisp + Layer 2 elisp");

    // Single Session shared by every probe so `(setq ...)` / `(require ...)`
    // performed by an earlier probe is visible to a later one.
    let mut session = Session::new();

    // ---- Section 1: runtime probes (= Phase 1.C.1 carry-over) -------------
    // Layout: title / header row / dashes-separator / probe rows.
    // The dashes go on a row BY THEMSELVES — earlier we wrote them
    // onto the header row first and then `put_probe_row' on the same
    // row, which left dashes visible between the column labels.
    g.put_str(4, 2, "Runtime probes");
    put_probe_row(&mut g, 5, "label", "form", "=>");
    for c in 2..COLS - 2 {
        g.put(6, c, '-');
    }
    let runtime_probes: &[(&str, &str)] = &[
        ("integer arithmetic", "(+ 1 2)"),
        ("multiplication",     "(* 7 8)"),
        ("nested call",        "(+ (* 3 4) (* 5 6))"),
        ("car . cdr",          "(car (cdr '(a b c)))"),
    ];
    for (i, (label, form)) in runtime_probes.iter().enumerate() {
        let row = 7 + i;
        let result = session.eval_to_string(form);
        put_probe_row(&mut g, row, label, form, &result);
    }

    // ---- Section 2: Layer 2 probes ---------------------------------------
    g.put_str(12, 2, "Layer 2 probes (load-path + require)");
    put_probe_row(&mut g, 13, "label", "form", "=>");
    for c in 2..COLS - 2 {
        g.put(14, c, '-');
    }

    // Phase 1.C.2 setup: prime the load-path before any require.
    let setup = nelisp_bridge::layer2_setup_form();
    let setup_result = session.eval_to_string(&setup);
    put_probe_row(&mut g, 15, "load-path setup", "(setq load-path …)", &setup_result);

    // require + post-require fboundp checks.
    let probes_after_setup: &[(&str, &str)] = &[
        ("require emacs-error",  "(require 'emacs-error)"),
        ("fboundp user-error",   "(fboundp 'user-error)"),
        ("fboundp define-error", "(fboundp 'define-error)"),
        ("define-error works",
         "(progn (define-error 'my-test \"my\") (get 'my-test 'error-message))"),
        ("user-error catches",
         "(condition-case e (user-error \"boom\") (user-error (cadr e)))"),
    ];
    for (i, (label, form)) in probes_after_setup.iter().enumerate() {
        let row = 16 + i;
        let result = session.eval_to_string(form);
        put_probe_row(&mut g, row, label, form, &result);
    }

    // Footer.
    g.put_str_centered(last_row - 1, "Layer 2 elisp loaded into a single embedded NeLisp session");
    g.put_str_centered(last_row, " close X to quit ");

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
