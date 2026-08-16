//! The application entry point. Everything it drives lives in the library beside it.

use adw::prelude::*;
use gtk4::gdk::Display;

use sg_linux::{palette, store, window};

const APP_ID: &str = "cloud.lazarev.ServerGlass";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| {
        // The stylesheet is loaded once, against the display rather than a widget, so every window
        // and every dialog gets it.
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(palette::STYLE);
        if let Some(display) = Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });

    app.connect_activate(|app| {
        let paths = store::Paths::from_env();
        let window = window::Window::new(app, paths);
        window.present();
        // The window owns everything reachable from it, and the application owns the widgets. This
        // leak is deliberate: the `Rc<Window>` would otherwise be dropped at the end of this
        // callback while GTK still holds the tree, and every timer callback would find its state
        // gone.
        std::mem::forget(window);
    });

    app.run()
}
