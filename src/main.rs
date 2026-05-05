// nemacs-gtk — Layer 3 GTK4 GUI display backend for nelisp-emacs.
//
// Phase 2 architecture: this binary is the **boot stub**.  It:
//   1. Brings up a NeLisp Session,
//   2. Bootstraps Layer 2 via `(require 'emacs-init)`,
//   3. Sets `emacs-display-system' to `'gtk' so substrate
//      `(window-system)' / `(display-graphic-p)' return the GUI path,
//   4. Registers the `nelisp-gtk-*' GTK4 builtins (= grid put / draw /
//      key poll / iterate / quit) against the Session's `Env',
//   5. Hands control to the elisp frontend
//      (`nemacs-gtk-frontend.el' on the substrate side) which drives
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

    // 1. Layer 2 substrate.
    let r = session.eval_to_string(&nelisp_bridge::layer2_setup_form());
    if r.starts_with("ERR ") {
        eprintln!("[nemacs-gtk] layer2 boot failed: {r}");
        eprintln!(
            "[nemacs-gtk] layer2_src_path = {}",
            nelisp_bridge::layer2_src_path()
        );
        std::process::exit(1);
    }

    // 2. Display-system flip — substrate Phase 1.E surface.
    let r = session.eval_to_string(
        "(progn (setq emacs-display-system 'gtk)
                (setq initial-window-system 'gtk)
                'gtk)",
    );
    if r.starts_with("ERR ") {
        eprintln!("[nemacs-gtk] display-system flip failed: {r}");
        std::process::exit(1);
    }

    // 3. GTK builtins.
    let state = Rc::new(RefCell::new(GtkState::new()));
    gtk_backend::register_all(session.env_mut(), state.clone());

    // 4. Hand off to the elisp frontend.
    let r = session.eval_to_string("(require 'nemacs-gtk-frontend)");
    if r.starts_with("ERR ") {
        eprintln!("[nemacs-gtk] frontend require failed: {r}");
        eprintln!(
            "[nemacs-gtk] (expected substrate file: {}/nemacs-gtk-frontend.el)",
            nelisp_bridge::layer2_src_path()
        );
        std::process::exit(1);
    }

    let r = session.eval_to_string("(nemacs-gtk-main)");
    if r.starts_with("ERR ") {
        eprintln!("[nemacs-gtk] main loop exited with error: {r}");
        std::process::exit(1);
    }
}
