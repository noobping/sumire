#[cfg(target_os = "linux")]
#[cfg(feature = "setup")]
mod setup;

mod config;
mod listen;
mod meta;
mod station;
mod ui;
mod http_source;

use crate::config::{APP_ID, RESOURCE_ID};
use adw::prelude::*;
use adw::Application;
use gtk::{gdk::Display, IconTheme};

fn main() {
    // Register resources compiled into the binary. If this fails, the app cannot find its assets.
    gtk::gio::resources_register_include!("compiled.gresource")
        .expect("Failed to register resources");

    // Initialize libadwaita/GTK. This must be called before any UI code.
    adw::init().expect("Failed to initialize libadwaita");

    // Load the icon theme from the embedded resources so that icons resolve correctly even outside a installed environment.
    if let Some(display) = Display::default() {
        let theme = IconTheme::for_display(&display);
        theme.add_resource_path(RESOURCE_ID);
    }

    // Create the GTK application. The application ID must be unique and corresponds to the desktop file name.
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_ui); // Build the UI when the application is activated.
    app.run(); // Run the application. This function does not return until the last window is closed.
}
