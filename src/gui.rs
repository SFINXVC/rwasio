use adw::Application;
use adw::glib;
use adw::prelude::*;
use gtk4::Orientation;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

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

fn stat_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build()
}

fn status_row(title: &str) -> (adw::ActionRow, gtk4::Label) {
    let label = gtk4::Label::builder().label("● Inactive").build();
    label.add_css_class("dim-label");

    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(&label);
    (row, label)
}

fn apply_status(label: &gtk4::Label, active: bool) {
    if active {
        label.set_label("● Active");
        label.remove_css_class("dim-label");
        label.add_css_class("success");
    } else {
        label.set_label("● Inactive");
        label.remove_css_class("success");
        label.add_css_class("dim-label");
    }
}

struct DebugWidgets {
    out_status: gtk4::Label,
    in_status: gtk4::Label,
    buf_idx: adw::ActionRow,
    asio_buf: adw::ActionRow,
    asio_in_ch: adw::ActionRow,
    asio_out_ch: adw::ActionRow,
    cap_cbs: adw::ActionRow,
    cap_frames: adw::ActionRow,
    cap_staging: adw::ActionRow,
    out_cbs: adw::ActionRow,
    out_frames: adw::ActionRow,
}

fn refresh_debug(w: &DebugWidgets) {
    let out_running = crate::PW_STREAM_SENDER
        .read()
        .map(|g| g.is_some())
        .unwrap_or(false);
    let in_running = crate::PW_INPUT_STREAM_SENDER
        .read()
        .map(|g| g.is_some())
        .unwrap_or(false);

    apply_status(&w.out_status, out_running);
    apply_status(&w.in_status, in_running);

    w.buf_idx.set_subtitle(
        &crate::DBG_CURRENT_BUFFER_IDX
            .load(Ordering::Relaxed)
            .to_string(),
    );

    let buf = crate::DBG_ASIO_BUFFER_SIZE.load(Ordering::Relaxed);
    w.asio_buf.set_subtitle(&if buf > 0 {
        format!("{} samples", buf)
    } else {
        "—".into()
    });

    let in_ch = crate::DBG_NUM_INPUTS.load(Ordering::Relaxed);
    w.asio_in_ch.set_subtitle(&if in_ch > 0 {
        in_ch.to_string()
    } else {
        "—".into()
    });

    let out_ch = crate::DBG_NUM_OUTPUTS.load(Ordering::Relaxed);
    w.asio_out_ch.set_subtitle(&if out_ch > 0 {
        out_ch.to_string()
    } else {
        "—".into()
    });

    let cap_cbs = crate::DBG_CAPTURE_CALLBACKS.load(Ordering::Relaxed);
    w.cap_cbs.set_subtitle(&cap_cbs.to_string());

    let cap_frames = crate::DBG_CAPTURE_FRAMES.load(Ordering::Relaxed);
    w.cap_frames.set_subtitle(&if cap_cbs > 0 {
        cap_frames.to_string()
    } else {
        "—".into()
    });

    let staging = crate::DBG_STAGING_SAMPLES.load(Ordering::Relaxed);
    w.cap_staging.set_subtitle(&if staging > 0 {
        format!("{} samples", staging)
    } else {
        "—".into()
    });

    let out_cbs = crate::DBG_OUTPUT_CALLBACKS.load(Ordering::Relaxed);
    w.out_cbs.set_subtitle(&out_cbs.to_string());

    let out_frames = crate::DBG_OUTPUT_FRAMES.load(Ordering::Relaxed);
    w.out_frames.set_subtitle(&if out_cbs > 0 {
        out_frames.to_string()
    } else {
        "—".into()
    });
}

fn build_debug_page() -> adw::NavigationPage {
    let (out_status_row, out_status_lbl) = status_row("Output Stream");
    let (in_status_row, in_status_lbl) = status_row("Input Stream");
    let buf_idx_row = stat_row("Buffer Index", "—");

    let status_group = adw::PreferencesGroup::builder()
        .title("Stream Status")
        .build();
    status_group.add(&out_status_row);
    status_group.add(&in_status_row);
    status_group.add(&buf_idx_row);

    let asio_buf_row = stat_row("Buffer Size", "—");
    let asio_rate_row = stat_row("Sample Rate", "44100 Hz");
    let asio_in_row = stat_row("Input Channels", "—");
    let asio_out_row = stat_row("Output Channels", "—");

    let asio_group = adw::PreferencesGroup::builder().title("ASIO").build();
    asio_group.add(&asio_buf_row);
    asio_group.add(&asio_rate_row);
    asio_group.add(&asio_in_row);
    asio_group.add(&asio_out_row);

    let cap_cbs_row = stat_row("Callbacks", "0");
    let cap_frames_row = stat_row("Frames (last)", "—");
    let cap_staging_row = stat_row("Ring Occupancy", "—");

    let capture_group = adw::PreferencesGroup::builder()
        .title("PipeWire Capture")
        .build();
    capture_group.add(&cap_cbs_row);
    capture_group.add(&cap_frames_row);
    capture_group.add(&cap_staging_row);

    let out_cbs_row = stat_row("Callbacks", "0");
    let out_frames_row = stat_row("Frames (last)", "—");

    let output_group = adw::PreferencesGroup::builder()
        .title("PipeWire Output")
        .build();
    output_group.add(&out_cbs_row);
    output_group.add(&out_frames_row);

    let vbox = gtk4::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    vbox.append(&status_group);
    vbox.append(&asio_group);
    vbox.append(&capture_group);
    vbox.append(&output_group);

    let scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&vbox)
        .build();

    let reset_btn = gtk4::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Reset counters")
        .build();
    reset_btn.connect_clicked(|_| {
        crate::DBG_CAPTURE_CALLBACKS.store(0, Ordering::Relaxed);
        crate::DBG_OUTPUT_CALLBACKS.store(0, Ordering::Relaxed);
    });

    let header = adw::HeaderBar::new();
    header.pack_end(&reset_btn);

    let toolbar = adw::ToolbarView::new();
    toolbar.set_top_bar_style(adw::ToolbarStyle::Raised);
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));

    let page = adw::NavigationPage::builder()
        .title("Diagnostics")
        .tag("debug")
        .child(&toolbar)
        .build();

    let widgets = Rc::new(DebugWidgets {
        out_status: out_status_lbl,
        in_status: in_status_lbl,
        buf_idx: buf_idx_row,
        asio_buf: asio_buf_row,
        asio_in_ch: asio_in_row,
        asio_out_ch: asio_out_row,
        cap_cbs: cap_cbs_row,
        cap_frames: cap_frames_row,
        cap_staging: cap_staging_row,
        out_cbs: out_cbs_row,
        out_frames: out_frames_row,
    });

    let source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    {
        let w = widgets.clone();
        let src = source.clone();
        page.connect_map(move |_| {
            let w = w.clone();
            let id = glib::timeout_add_local(Duration::from_millis(250), move || {
                refresh_debug(&w);
                glib::ControlFlow::Continue
            });
            *src.borrow_mut() = Some(id);
        });
    }

    {
        let src = source.clone();
        page.connect_unmap(move |_| {
            if let Some(id) = src.borrow_mut().take() {
                id.remove();
            }
        });
    }

    page
}

fn build_window(app: &Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Rusty Wine ASIO")
        .default_width(500)
        .default_height(600)
        .resizable(false)
        .build();

    let nav_view = adw::NavigationView::new();

    let debug_page = build_debug_page();

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
        crate::driver::set_input_target(id);
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

    let diag_btn = gtk4::Button::builder()
        .icon_name("dialog-information-symbolic")
        .tooltip_text("Diagnostics")
        .build();
    let nav_weak = nav_view.downgrade();
    diag_btn.connect_clicked(move |_| {
        if let Some(nav) = nav_weak.upgrade() {
            nav.push_by_tag("debug");
        }
    });
    header_bar.pack_start(&diag_btn);

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

    let main_toolbar = adw::ToolbarView::new();
    main_toolbar.set_top_bar_style(adw::ToolbarStyle::Raised);
    main_toolbar.add_top_bar(&header_bar);
    main_toolbar.set_content(Some(&scroll));

    let main_page = adw::NavigationPage::builder()
        .title("Rusty Wine ASIO")
        .tag("main")
        .child(&main_toolbar)
        .build();

    nav_view.add(&main_page);
    nav_view.add(&debug_page);

    window.set_content(Some(&nav_view));
    window
}
