// nelisp-gtk — GTK4 substrate for NeLisp / elisp.  Currently drives the
// Emacs-shaped frontend on top of nelisp-emacs (Layer 2/3), but the GTK
// backend itself is intentionally generic so non-Emacs nelisp/elisp apps
// can grow on top of the same `nelisp-gtk-*' builtins.
//
// Phase 2 architecture: this binary is the **boot stub**.  It:
//   1. Brings up a NeLisp Session,
//   2. Bootstraps Layer 2 via `(require 'emacs-init)`,
//   3. Sets `emacs-display-system' to `'gtk' so substrate
//      `(window-system)' / `(display-graphic-p)' return the GUI path,
//   4. Registers the `nelisp-gtk-*' GTK4 builtins (= grid put / draw /
//      key poll / iterate / quit) against the Session's `Env',
//   5. Hands control to the elisp frontend
//      (`nelisp-gtk-frontend.el' on the substrate side) which drives
//      the main loop, mode-line composition, and key dispatch.
//
// Everything visual / behavioural lives in elisp.  This file is GTK
// plumbing + boot only.

mod grid;
mod gtk_backend;
mod nelisp_bridge;

use std::cell::RefCell;
use std::rc::Rc;

use gtk_backend::GtkState;
use nelisp_bridge::Session;

fn main() {
    let mut session = Session::new();
    let src = nelisp_bridge::layer2_setup_form();
    eprintln!(
        "[nelisp-gtk] layer2_src_path = {}",
        nelisp_bridge::layer2_src_path()
    );

    // 1. Layer 2 substrate.
    let r = session.eval_to_string(&src);
    eprintln!("[nelisp-gtk] layer2 setup = {r}");
    if r.starts_with("ERR ") {
        std::process::exit(1);
    }

    // 2. Display-system flip — substrate Phase 1.E surface.
    let r = session.eval_to_string(
        "(progn (setq emacs-display-system 'gtk)
                (setq initial-window-system 'gtk)
                'gtk)",
    );
    eprintln!("[nelisp-gtk] display setup = {r}");
    if r.starts_with("ERR ") {
        std::process::exit(1);
    }

    // 3. GTK builtins.
    let state = Rc::new(RefCell::new(GtkState::new()));
    gtk_backend::register_all(session.env_mut(), state.clone());
    eprintln!("[nelisp-gtk] register_all done");

    // 4. Frontend file diagnostics (= pin the load-path / file existence
    // so a "Cannot open load file" failure surfaces ground truth).
    let frontend_path = format!(
        "{}/nelisp-gtk-frontend.el",
        nelisp_bridge::layer2_src_path()
    );
    eprintln!(
        "[nelisp-gtk] frontend file exists on disk = {}",
        std::path::Path::new(&frontend_path).is_file()
    );
    eprintln!(
        "[nelisp-gtk] (member layer2_src_path load-path) = {}",
        session.eval_to_string(&format!(
            "(if (member \"{}\" load-path) t nil)",
            nelisp_bridge::layer2_src_path()
        ))
    );

    // 5. Hand off to the elisp frontend.
    let r = session.eval_to_string("(require 'nelisp-gtk-frontend)");
    eprintln!("[nelisp-gtk] require frontend = {r}");
    if r.starts_with("ERR ") {
        std::process::exit(1);
    }

    let r = session.eval_to_string("(nelisp-gtk-main)");
    eprintln!("[nelisp-gtk] main loop result = {r}");
    if r.starts_with("ERR ") {
        std::process::exit(1);
    }
}
