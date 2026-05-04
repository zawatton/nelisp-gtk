// Phase 1.B/1.C foundation — character-cell grid abstraction.
//
// Models the same logical surface as `emacs-tui-backend' (Layer 3 TUI):
// a fixed-size 2D buffer of `char' cells that the GUI back-end paints
// onto the window via Pango/Cairo at canonical (col*cell_w, row*cell_h)
// pixel positions.  Future phases will extend each cell with a face
// reference so 256-colour / truecolour faces render correctly.

#[derive(Debug, Clone)]
pub struct CharGrid {
    pub rows: usize,
    pub cols: usize,
    cells: Vec<char>,
}

impl CharGrid {
    pub fn filled(rows: usize, cols: usize, ch: char) -> Self {
        Self { rows, cols, cells: vec![ch; rows * cols] }
    }

    pub fn blank(rows: usize, cols: usize) -> Self {
        Self::filled(rows, cols, ' ')
    }

    pub fn put(&mut self, row: usize, col: usize, ch: char) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = ch;
        }
    }

    pub fn put_str(&mut self, row: usize, col: usize, s: &str) {
        for (i, ch) in s.chars().enumerate() {
            self.put(row, col + i, ch);
        }
    }

    pub fn put_str_centered(&mut self, row: usize, s: &str) {
        let len = s.chars().count();
        let start = self.cols.saturating_sub(len) / 2;
        self.put_str(row, start, s);
    }

    pub fn get(&self, row: usize, col: usize) -> char {
        self.cells[row * self.cols + col]
    }
}
