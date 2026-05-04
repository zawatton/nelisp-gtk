// Phase 1.A — minimal GTK4 hello window.
// Goal: verify the GTK4 build chain works on Linux + render a placeholder
// "Hello, NeLisp" string using Pango defaults.  No NeLisp embedding yet.

use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow, Label};

const APP_ID: &str = "org.nelisp.emacs.gtk";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let label = Label::builder()
        .label("Hello, NeLisp\n(Phase 1.A — GTK4 scaffolding)")
        .margin_top(40)
        .margin_bottom(40)
        .margin_start(40)
        .margin_end(40)
        .build();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("nemacs-gtk")
        .default_width(640)
        .default_height(400)
        .child(&label)
        .build();

    window.present();
}
