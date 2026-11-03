use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use gtk4::prelude::*;
use ksni::blocking::TrayMethods;
use t2newfanrd::{hwmon, ipc};

const APP_ID: &str = "io.github.anthony2cc.T2NewFanRD";

#[derive(Clone)]
struct FanRow {
    path: PathBuf,
    scale: gtk4::Scale,
    actual_label: gtk4::Label,
}

struct Tray {
    tx: Sender<Action>,
}

enum Action {
    Show,
    Quit,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "t2newfanrd".to_owned()
    }

    fn title(&self) -> String {
        "T2 Fan Control".to_owned()
    }

    fn icon_name(&self) -> String {
        "t2newfanrd-symbolic".to_owned()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(Action::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        vec![
            StandardItem {
                label: "Open fan controls".to_owned(),
                icon_name: "preferences-system".to_owned(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.send(Action::Show);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.tx.send(Action::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn main() {
    let fan_paths = hwmon::find_fan_paths();
    let (tx, rx) = mpsc::channel();

    let tray_handle = match (Tray { tx })
        .assume_sni_available(true)
        .spawn()
    {
        Ok(handle) => Some(handle),
        Err(err) => {
            eprintln!("Unable to register system tray icon: {err}");
            None
        }
    };
    let show_on_start = !std::env::args().any(|arg| arg == "--hidden") || tray_handle.is_none();

    let receiver = Rc::new(RefCell::new(Some(rx)));
    let app = gtk4::Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let Some(receiver) = receiver.borrow_mut().take() else {
            return;
        };
        let window = build_window(app, fan_paths.clone(), receiver);
        if show_on_start {
            window.present();
        }
    });

    app.run_with_args::<&str>(&[]);
    drop(tray_handle);
}

fn build_window(
    app: &gtk4::Application,
    fan_paths: Vec<PathBuf>,
    receiver: Receiver<Action>,
) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("T2 Fan Control")
        .default_width(460)
        .build();

    let header = gtk4::HeaderBar::new();
    let header_title = gtk4::Label::new(Some("T2 Fan Control"));
    header_title.add_css_class("title-4");
    header.set_title_widget(Some(&header_title));
    window.set_titlebar(Some(&header));

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    window.set_child(Some(&content));

    let auto_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let auto_label = gtk4::Label::new(Some("Automatic fan curve"));
    auto_label.set_xalign(0.0);
    auto_label.set_hexpand(true);
    auto_row.append(&auto_label);

    let auto_switch = gtk4::Switch::new();
    auto_switch.set_active(true);
    auto_switch.set_tooltip_text(Some(
        "Turn off to hold the fans at the speeds selected below",
    ));
    auto_row.append(&auto_switch);
    content.append(&auto_row);

    content.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    let updating = Rc::new(Cell::new(false));
    let mut fans = Vec::with_capacity(fan_paths.len());

    for (index, path) in fan_paths.into_iter().enumerate() {
        let name = hwmon::read_text(&path, "_label").unwrap_or_else(|| format!("Fan {}", index + 1));
        let min = hwmon::read_u32(&path, "_min").unwrap_or(0);
        let max = hwmon::read_u32(&path, "_max").unwrap_or(min.max(1));

        let title = gtk4::Label::new(None);
        title.set_markup(&format!("<b>{}</b>", gtk4::glib::markup_escape_text(&name)));
        title.set_xalign(0.0);
        title.set_hexpand(true);

        let actual_label = gtk4::Label::new(Some("— RPM"));
        actual_label.add_css_class("dim-label");

        let title_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        title_row.append(&title);
        title_row.append(&actual_label);

        let scale = gtk4::Scale::with_range(
            gtk4::Orientation::Horizontal,
            f64::from(min),
            f64::from(max),
            1.0,
        );
        scale.set_digits(0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_value(f64::from(min));
        scale.set_sensitive(false);

        let fan_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        fan_box.append(&title_row);
        fan_box.append(&scale);
        content.append(&fan_box);

        fans.push(FanRow {
            path,
            scale,
            actual_label,
        });
    }

    let status_label = gtk4::Label::new(Some("Connecting…"));
    status_label.set_xalign(0.0);
    status_label.set_hexpand(true);
    status_label.set_wrap(true);
    status_label.add_css_class("dim-label");

    let quit_button = gtk4::Button::with_label("Quit");
    let app_for_quit = app.clone();
    quit_button.connect_clicked(move |_| app_for_quit.quit());

    let footer = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    footer.append(&status_label);
    footer.append(&quit_button);
    content.append(&footer);

    let scales: Vec<gtk4::Scale> = fans.iter().map(|fan| fan.scale.clone()).collect();

    {
        let scales = scales.clone();
        let updating = updating.clone();
        let auto_switch = auto_switch.clone();
        auto_switch.connect_active_notify(move |switch| {
            if updating.get() {
                return;
            }
            let command = if switch.is_active() {
                "AUTO".to_owned()
            } else {
                set_command(&scales)
            };
            let _ = ipc::send_command(&command);
        });
    }

    {
        let pending = Rc::new(Cell::new(false));
        for scale in &scales {
            let scales = scales.clone();
            let pending = pending.clone();
            let updating = updating.clone();
            scale.connect_value_changed(move |_| {
                if updating.get() || pending.get() {
                    return;
                }
                pending.set(true);
                let scales = scales.clone();
                let pending = pending.clone();
                gtk4::glib::timeout_add_local_once(Duration::from_millis(120), move || {
                    pending.set(false);
                    let _ = ipc::send_command(&set_command(&scales));
                });
            });
        }
    }

    {
        let fans = fans.clone();
        let auto_switch = auto_switch.clone();
        let status_label = status_label.clone();
        let updating = updating.clone();
        refresh(&fans, &auto_switch, &status_label, &updating);
        gtk4::glib::timeout_add_local(Duration::from_secs(1), move || {
            refresh(&fans, &auto_switch, &status_label, &updating);
            gtk4::glib::ControlFlow::Continue
        });
    }

    {
        let app = app.clone();
        let window_for_actions = window.clone();
        gtk4::glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(action) = receiver.try_recv() {
                match action {
                    Action::Show => window_for_actions.present(),
                    Action::Quit => app.quit(),
                }
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    window.connect_close_request(|window| {
        window.set_visible(false);
        gtk4::glib::Propagation::Stop
    });

    window
}

fn set_command(scales: &[gtk4::Scale]) -> String {
    let targets: Vec<u32> = scales
        .iter()
        .map(|scale| scale.value().round() as u32)
        .collect();
    format!(
        "SET {}",
        targets
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn refresh(fans: &[FanRow], auto_switch: &gtk4::Switch, status_label: &gtk4::Label, updating: &Cell<bool>) {
    let reply = ipc::send_command("STATUS");
    let mode = reply.ok().and_then(|reply| parse_status(&reply));

    match mode {
        Some(Some(targets)) => {
            updating.set(true);
            auto_switch.set_active(false);
            updating.set(false);
            auto_switch.set_sensitive(true);

            for (index, fan) in fans.iter().enumerate() {
                let target = targets
                    .get(index)
                    .copied()
                    .or_else(|| hwmon::read_u32(&fan.path, "_target"))
                    .unwrap_or(0);
                let (min, max) = (fan.scale.adjustment().lower(), fan.scale.adjustment().upper());
                let target = f64::from(target).clamp(min, max);

                updating.set(true);
                fan.scale.set_value(target);
                updating.set(false);

                fan.scale.set_sensitive(true);
                update_actual(fan);
            }
            status_label.set_text("Manual override · connected");
        }
        Some(None) => {
            updating.set(true);
            auto_switch.set_active(true);
            updating.set(false);
            auto_switch.set_sensitive(true);

            for fan in fans {
                let target = hwmon::read_u32(&fan.path, "_target")
                    .or_else(|| hwmon::read_u32(&fan.path, "_input"))
                    .unwrap_or(0);
                let (min, max) = (fan.scale.adjustment().lower(), fan.scale.adjustment().upper());

                updating.set(true);
                fan.scale.set_value(f64::from(target).clamp(min, max));
                updating.set(false);

                fan.scale.set_sensitive(false);
                update_actual(fan);
            }
            status_label.set_text("Automatic fan curve · connected");
        }
        None => {
            auto_switch.set_sensitive(false);
            for fan in fans {
                fan.scale.set_sensitive(false);
                update_actual(fan);
            }
            status_label.set_text("Daemon not reachable — is t2newfanrd.service running?");
        }
    }
}

fn update_actual(fan: &FanRow) {
    match hwmon::read_u32(&fan.path, "_input") {
        Some(rpm) => fan.actual_label.set_text(&format!("{rpm} RPM")),
        None => fan.actual_label.set_text("— RPM"),
    }
}

/// `AUTO` -> Some(None); `MANUAL 3000 2500` -> Some(Some([3000, 2500]))
fn parse_status(reply: &str) -> Option<Option<Vec<u32>>> {
    if reply == "AUTO" {
        return Some(None);
    }

    let rest = reply.strip_prefix("MANUAL ")?;
    let targets = rest
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(Some(targets))
}
