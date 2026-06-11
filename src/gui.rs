use libadwaita as adw;

use adw::prelude::*;
use adw::Application;
use gtk4::{Label, LinkButton, Box as GtkBox, Orientation, Align};
use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;

#[derive(Debug)]
pub enum GuiError {
    Startup,
}

static GUI_SENDER: OnceLock<SyncSender<()>> = OnceLock::new();

/// Launch the control panel window. Returns immediately.
pub fn show_control_panel() -> Result<(), GuiError> {
    crate::rlog!("[gui] opening control panel");

    let sender = GUI_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<()>(4);

        std::thread::spawn(move || {
            for () in rx {
                crate::rlog!("[gui] gui thread received panel request");

                let app = Application::builder()
                    .application_id("io.github.rwasio.ControlPanel")
                    .flags(adw::gio::ApplicationFlags::NON_UNIQUE)
                    .build();

                app.connect_activate(|app| {
                    let window = build_window(app);
                    let app_weak = app.downgrade();
                    window.connect_close_request(move |_| {
                        if let Some(app) = app_weak.upgrade() {
                            app.quit();
                        }
                        adw::glib::Propagation::Proceed
                    });
                    window.present();
                });

                let _exit = app.run();
                crate::rlog!("[gui] control panel closed");
            }
        });

        tx
    });

    sender.send(()).map_err(|_| {
        crate::rlog!("[gui] failed to send to gui thread");
        GuiError::Startup
    })?;

    Ok(())
}

fn build_window(app: &Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Rusty Wine ASIO")
        .default_width(400)
        .default_height(280)
        .resizable(false)
        .build();

    let vbox = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .valign(Align::Center)
        .halign(Align::Center)
        .margin_top(32)
        .margin_bottom(32)
        .margin_start(32)
        .margin_end(32)
        .build();

    let title = Label::builder()
        .label("Rusty Wine ASIO")
        .css_classes(["title-1"])
        .halign(Align::Center)
        .build();

    let description = Label::builder()
        .label("A Fast ASIO driver for Wine that wires directly to JACK or PipeWire, written in Rust.")
        .css_classes(["body"])
        .halign(Align::Center)
        .justify(gtk4::Justification::Center)
        .wrap(true)
        .build();

    let copyright = Label::builder()
        .label("Copyright \u{00A9} 2026 SFINXV. All rights reserved.")
        .css_classes(["dim-label"])
        .halign(Align::Center)
        .build();

    let github = LinkButton::builder()
        .label("github.com/SFINXVC/rwasio")
        .uri("https://github.com/SFINXVC/rwasio")
        .halign(Align::Center)
        .build();

    vbox.append(&title);
    vbox.append(&description);
    vbox.append(&copyright);
    vbox.append(&github);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());
    toolbar_view.set_content(Some(&vbox));

    window.set_content(Some(&toolbar_view));
    window
}
