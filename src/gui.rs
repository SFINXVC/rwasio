use adw::Application;
use adw::prelude::*;
use gtk4::Orientation;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::mpsc::{self, SyncSender};

use crate::DEVICE_LIST;

#[derive(Debug)]
pub enum GuiError {
    Startup,
}

static GUI_SENDER: OnceLock<SyncSender<()>> = OnceLock::new();

pub fn show_control_panel() -> Result<(), GuiError> {
    crate::rlog!("[gui] opening control panel");

    let sender = GUI_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<()>(4);

        std::thread::spawn(move || {
            for () in rx {
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

                app.run();
            }
        });

        tx
    });

    sender.send(()).map_err(|_| GuiError::Startup)
}

fn make_device_selector(
    title: &str,
    options: &[(&str, &str)],
    default: usize,
    on_select: impl Fn(&str) + 'static,
) -> adw::ExpanderRow {
    let expander = adw::ExpanderRow::builder()
        .title(title)
        .subtitle(
            options
                .get(default)
                .map(|(name, _)| *name)
                .unwrap_or("None"),
        )
        .build();

    let checks: Rc<RefCell<Vec<gtk4::Image>>> = Rc::new(RefCell::new(Vec::new()));
    let on_select = Rc::new(on_select);

    for (i, &(name, id)) in options.iter().enumerate() {
        let row = adw::ActionRow::builder()
            .title(name)
            .activatable(true)
            .build();

        let check = gtk4::Image::from_icon_name("object-select-symbolic");
        check.set_visible(i == default);
        row.add_suffix(&check);

        let exp = expander.clone();
        let label = name.to_string();
        let id = id.to_string();
        let checks_clone = Rc::clone(&checks);
        let on_select_clone = Rc::clone(&on_select);

        row.connect_activated(move |_| {
            for c in checks_clone.borrow().iter() {
                c.set_visible(false);
            }
            if let Some(c) = checks_clone.borrow().get(i) {
                c.set_visible(true);
            }
            exp.set_subtitle(&label);
            exp.set_expanded(false);
            on_select_clone(&id);
            crate::rlog!("[gui] {} selected: {} (id={})", exp.title(), label, id);
        });

        checks.borrow_mut().push(check);
        expander.add_row(&row);
    }

    expander
}

fn make_buffer_selector(default: usize) -> adw::ComboRow {
    let options = gtk4::StringList::new(&["64", "128", "256", "512", "1024", "2048"]);
    adw::ComboRow::builder()
        .title("Buffer size")
        .model(&options)
        .selected(default as u32)
        .build()
}

fn build_window(app: &Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Rusty Wine ASIO")
        .default_width(500)
        .default_height(600)
        .resizable(false)
        .build();

    let (sink_names, source_names) = DEVICE_LIST
        .get()
        .map(|(s, r)| (s.as_slice(), r.as_slice()))
        .unwrap_or((&[], &[]));

    let mut sink_opts: Vec<(&str, &str)> = vec![("System Default", "")];
    sink_opts.extend(sink_names.iter().map(|(n, id)| (n.as_str(), id.as_str())));

    let mut source_opts: Vec<(&str, &str)> = vec![("System Default", "")];
    source_opts.extend(source_names.iter().map(|(n, id)| (n.as_str(), id.as_str())));

    let output_selector = make_device_selector("Output device", &sink_opts, 0, |id| {
        if let Ok(mut w) = crate::SELECTED_SINK.write() {
            *w = id.to_string();
        }
        crate::driver::set_output_target(id);
        crate::rlog!("[gui] selected sink id: {}", id);
    });

    let input_selector = make_device_selector("Input device", &source_opts, 0, |id| {
        if let Ok(mut w) = crate::SELECTED_SOURCE.write() {
            *w = id.to_string();
        }
        crate::rlog!("[gui] selected source id: {}", id);
    });

    let device_group = adw::PreferencesGroup::builder().title("Devices").build();
    device_group.add(&output_selector);
    device_group.add(&input_selector);

    let buffer_selector = make_buffer_selector(2);
    buffer_selector.connect_selected_notify(|row| {
        const SIZES: [i32; 6] = [64, 128, 256, 512, 1024, 2048];
        if let Some(&size) = SIZES.get(row.selected() as usize) {
            crate::driver::set_preferred_buffer_size(size);
        }
    });
    let perf_group = adw::PreferencesGroup::builder()
        .title("Performance")
        .build();
    perf_group.add(&buffer_selector);

    let vbox = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    vbox.append(&device_group);
    vbox.append(&perf_group);

    let scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&vbox)
        .build();

    let header_bar = adw::HeaderBar::builder()
        .decoration_layout(":close")
        .build();

    let menu = adw::gio::Menu::new();
    menu.append(Some("About"), Some("win.about"));

    let menu_btn = gtk4::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&menu)
        .build();

    header_bar.pack_end(&menu_btn);

    let about_action = adw::gio::SimpleAction::new("about", None);
    let window_weak = window.downgrade();
    about_action.connect_activate(move |_, _| {
        let Some(win) = window_weak.upgrade() else {
            return;
        };
        let dialog = adw::AboutDialog::builder()
            .application_name("Rusty Wine ASIO")
            .version("0.1.0")
            .developer_name("SFINXV")
            .website("https://github.com/SFINXVC/rwasio")
            .build();
        dialog.present(Some(&win));
    });
    window.add_action(&about_action);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.set_top_bar_style(adw::ToolbarStyle::Raised);
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&scroll));

    window.set_content(Some(&toolbar_view));
    window
}
