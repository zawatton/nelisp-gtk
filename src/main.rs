// Phase 1.B — Pango monospace character grid (80 cols x 24 rows).
//
// This step replaces the placeholder Label with a DrawingArea that
// renders a fixed test pattern via Pango/Cairo at canonical character-cell
// positions, mirroring how `emacs-tui-backend' lays glyphs onto the
// terminal grid.  A future Phase 1.C will replace the static pattern
// with a logical-buffer redraw driven by the embedded NeLisp runtime.
//
// Test pattern (= visually verifiable Phase 1.B close gate):
//   - Row 0: header label
//   - Row 1: column ruler (0123456789 repeating)
//   - Rows 2..22: light dot fill ('.') everywhere except a diagonal '*'
//                 from top-left to bottom-right of the inner area
//   - Row 23: footer label
//   - Corner markers '+' at the four extreme cells
//
// Cell dimensions are measured once from Pango metrics for the chosen
// monospace font; the window opens at exactly the grid's pixel extent.

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

/// Measure the average advance width and total line height of the
/// chosen monospace font, returning (cell_w, cell_h, ascent) in pixels.
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

/// Build the test pattern character for a given (row, col).
fn pattern_char(row: usize, col: usize) -> char {
    let last_row = ROWS - 1;
    let last_col = COLS - 1;
    let inner_top = 2;
    let inner_bot = last_row - 1;

    // Corner markers
    if (row == 0 || row == last_row) && (col == 0 || col == last_col) {
        return '+';
    }

    // Header
    if row == 0 {
        let text = " Phase 1.B  Pango monospace char grid  80x24 ";
        let start = (COLS.saturating_sub(text.len())) / 2;
        let bytes = text.as_bytes();
        if col >= start && col < start + bytes.len() {
            return bytes[col - start] as char;
        }
        return '-';
    }

    // Column ruler
    if row == 1 {
        return char::from_digit((col % 10) as u32, 10).unwrap_or(' ');
    }

    // Footer
    if row == last_row {
        let text = " close X to quit ";
        let start = (COLS.saturating_sub(text.len())) / 2;
        let bytes = text.as_bytes();
        if col >= start && col < start + bytes.len() {
            return bytes[col - start] as char;
        }
        return '-';
    }

    // Inner area: '*' on the rough diagonal, '.' elsewhere.
    if row >= inner_top && row <= inner_bot {
        let inner_rows = inner_bot - inner_top + 1;
        let inner_cols = last_col + 1;
        let r = row - inner_top;
        let diag_col = (r * inner_cols) / inner_rows;
        if col == diag_col {
            return '*';
        }
        return '.';
    }
    ' '
}

fn build_ui(app: &Application) {
    let (cell_w, cell_h, ascent) = measure_cell();
    let canvas_w = (cell_w * COLS as f64).ceil() as i32;
    let canvas_h = (cell_h * ROWS as f64).ceil() as i32;

    let area = DrawingArea::new();
    area.set_content_width(canvas_w);
    area.set_content_height(canvas_h);

    area.set_draw_func(move |_area, cr, _w, _h| {
        // Background — pure white for the MVP, will become face-driven.
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();

        cr.set_source_rgb(0.0, 0.0, 0.0);
        let layout = pangocairo::functions::create_layout(cr);
        let desc = FontDescription::from_string(FONT);
        layout.set_font_description(Some(&desc));

        let mut buf = [0u8; 4];
        for row in 0..ROWS {
            for col in 0..COLS {
                let ch = pattern_char(row, col);
                if ch == ' ' {
                    continue;
                }
                layout.set_text(ch.encode_utf8(&mut buf));
                let x = col as f64 * cell_w;
                // Pango layouts draw from the top of the line; align the
                // glyph baseline to (row * cell_h + ascent) by moving to
                // the top of the cell row.
                let y = row as f64 * cell_h;
                cr.move_to(x, y);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
        let _ = ascent; // reserved for baseline-precise rendering in 1.C
    });

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .resizable(false)
        .child(&area)
        .build();
    window.present();
}
