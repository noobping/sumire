use adw::glib;
use adw::gtk::{
    self,
    gdk::Display,
    gio::SimpleAction,
    prelude::{ActionMapExt, GtkApplicationExt, GtkWindowExt, WidgetExt},
    ApplicationWindow, Button,
};
use adw::{prelude::*, Application, WindowTitle};
use gettextrs::gettext;
#[cfg(all(target_os = "linux", feature = "controls"))]
use souvlaki::{MediaControlEvent, MediaControls, MediaPlayback, PlatformConfig};
#[cfg(all(target_os = "linux", feature = "controls"))]
use std::{cell::RefCell, sync::mpsc};
use std::rc::Rc;

use crate::listen::Listen;
use crate::meta::Meta;
use crate::station::Station;

#[cfg(debug_assertions)]
const APP_ID: &str = "io.github.noobping.listenmoe_develop";
#[cfg(not(debug_assertions))]
const APP_ID: &str = "io.github.noobping.listenmoe";

fn make_action<F>(name: &str, f: F) -> SimpleAction
where
    F: Fn() + 'static,
{
    let action = SimpleAction::new(name, None);
    action.connect_activate(move |_, _| f());
    action
}

#[cfg(all(target_os = "linux", feature = "controls"))]
pub fn build_controls(
    window: &ApplicationWindow,
    app: &Application,
    win_title: &WindowTitle,
    play_button: &Button,
    pause_button: &Button,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) -> (
    Rc<RefCell<MediaControls>>,
    mpsc::Receiver<MediaControlEvent>,
) {
    let platform_config = PlatformConfig {
        display_name: "Listen Moe",
        dbus_name: APP_ID,
        hwnd: None,
    };
    let controls = MediaControls::new(platform_config).expect("Failed to init media controls");
    let controls = Rc::new(RefCell::new(controls));
    let (ctrl_tx, ctrl_rx) = mpsc::channel::<MediaControlEvent>();
    let tx = ctrl_tx.clone();
    controls
        .borrow_mut()
        .attach(move |event| {
            let _ = tx.send(event);
        })
        .expect("Failed to attach media control events");
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let pause = pause_button.clone();
        let controls = controls.clone();
        make_action("play", move || {
            win.set_title("Listen Moe");
            win.set_subtitle("Connecting...");
            meta.start();
            radio.start();
            play.set_visible(false);
            pause.set_visible(true);
            let _ = controls
                .borrow_mut()
                .set_playback(MediaPlayback::Playing { progress: None });
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let pause = pause_button.clone();
        let controls = controls.clone();
        make_action("pause", move || {
            meta.pause();
            radio.pause();
            pause.set_visible(false);
            play.set_visible(true);
            win.set_title("Listen Moe");
            win.set_subtitle(&gettext("J-POP and K-POP radio"));
            let _ = controls
                .borrow_mut()
                .set_playback(MediaPlayback::Paused { progress: None });
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = pause_button.clone();
        let controls = controls.clone();
        make_action("stop", move || {
            meta.stop();
            radio.stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("Listen Moe");
            win.set_subtitle(&gettext("J-POP and K-POP radio"));
            let _ = controls
                .borrow_mut()
                .set_playback(MediaPlayback::Paused { progress: None });
        })
    });
    add_actions(window, win_title, play_button, pause_button, radio, meta);
    add_accels(app);

    (controls, ctrl_rx)
}

#[cfg(not(all(target_os = "linux", feature = "controls")))]
pub fn build_actions(
    window: &ApplicationWindow,
    app: &Application,
    win_title: &WindowTitle,
    play_button: &Button,
    pause_button: &Button,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) {
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let pause = pause_button.clone();
        make_action("play", move || {
            win.set_title("Listen Moe");
            win.set_subtitle("Connecting...");
            meta.start();
            radio.start();
            play.set_visible(false);
            pause.set_visible(true);
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let pause = pause_button.clone();
        make_action("pause", move || {
            meta.pause();
            radio.pause();
            pause.set_visible(false);
            play.set_visible(true);
            win.set_title("Listen Moe");
            win.set_subtitle(&gettext("J-POP and K-POP radio"));
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = pause_button.clone();
        make_action("stop", move || {
            meta.stop();
            radio.stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("Listen Moe");
            win.set_subtitle(&gettext("J-POP and K-POP radio"));
        })
    });
    add_actions(window, win_title, play_button, pause_button, radio, meta);
    add_accels(app);
}

fn add_actions(
    window: &ApplicationWindow,
    win_title: &WindowTitle,
    play_button: &Button,
    pause_button: &Button,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) {
    window.add_action(&{
        let win = window.clone();
        make_action("quit", move || win.close())
    });
    window.add_action(&{
        let win_clone = window.clone();
        make_action("about", move || {
            let authors: Vec<_> = env!("CARGO_PKG_AUTHORS").split(':').collect();
            let homepage = option_env!("CARGO_PKG_HOMEPAGE").unwrap_or("");
            let issues = format!("{}/issues", env!("CARGO_PKG_REPOSITORY"));
            let comments = gettext(
                "It is time to ditch other radios. Stream and metadata provided by LISTEN.moe.",
            );
            let about = adw::AboutDialog::builder()
                .application_name("Listen Moe")
                .application_icon(APP_ID)
                .version(env!("CARGO_PKG_VERSION"))
                .developers(&authors[..])
                .translator_credits(gettext("AI translation (GPT-5.2); reviewed by nobody"))
                .website(homepage)
                .issue_url(issues)
                .support_url(format!("{}discord", homepage))
                .license_type(gtk::License::MitX11)
                .comments(comments)
                .build();
            about.present(Some(&win_clone));
        })
    });
    window.add_action(&{
        let play = play_button.clone();
        let pause = pause_button.clone();
        let win_clone = window.clone();
        make_action("toggle", move || {
            if play.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.play",
                    None::<&glib::Variant>,
                );
            } else if pause.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.pause",
                    None::<&glib::Variant>,
                );
            }
        })
    });
    window.add_action(&{
        let win = win_title.clone();
        make_action("copy", move || {
            let artist = win.title();
            let title = win.subtitle();
            if artist.is_empty() && title.is_empty() {
                return;
            }
            let text = if artist.is_empty() {
                title.to_string()
            } else if title.is_empty() {
                artist.to_string()
            } else {
                format!("{artist}, {title}")
            };
            if let Some(display) = Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&text);
            }
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win_clone = window.clone();
        let play = play_button.clone();
        make_action("next_station", move || {
            if play.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.play",
                    None::<&glib::Variant>,
                );
                return;
            }
            let current = radio.get_station();
            let next = other_station(current);
            radio.set_station(next);
            meta.set_station(next);
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let play = play_button.clone();
        make_action("prev_station", move || {
            if play.is_visible() {
                return; // paused -> do nothing
            }
            let current = radio.get_station();
            let prev = other_station(current);
            radio.set_station(prev);
            meta.set_station(prev);
        })
    });
}

fn add_accels(app: &Application) {
    app.set_accels_for_action("win.about", &["F1"]);
    app.set_accels_for_action("win.copy", &["<primary>c"]);
    app.set_accels_for_action("win.jpop", &["<primary>j"]);
    app.set_accels_for_action("win.kpop", &["<primary>k"]);
    app.set_accels_for_action("win.quit", &["<primary>q", "Escape"]);
    app.set_accels_for_action("win.prev_station", &["<primary>z", "XF86AudioPrev"]);
    app.set_accels_for_action(
        "win.next_station",
        &["<primary>y", "<primary><shift>z", "XF86AudioNext"],
    );
    app.set_accels_for_action(
        "win.toggle",
        &["<primary>p", "space", "Return", "<primary>s"],
    );
    app.set_accels_for_action("win.play", &["XF86AudioPlay"]);
    app.set_accels_for_action("win.stop", &["XF86AudioStop"]);
    app.set_accels_for_action("win.pause", &["XF86AudioPause"]);
}

pub fn populate_menu(
    window: &ApplicationWindow,
    play_button: &Button,
    menu: &gtk::gio::Menu,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) {
    menu.append(Some(&gettext("Copy title & artist")), Some("win.copy"));
    for station in [Station::Jpop, Station::Kpop] {
        let action = create_station_action(station, &play_button, &window, &radio, &meta);
        window.add_action(&action);
        menu.append(
            Some(
                gettext("Play %s")
                    .replace("%s", station.display_name())
                    .as_str(),
            ),
            Some(&format!("win.{}", station.name())),
        );
    }
    menu.append(Some(&gettext("About")), Some("win.about"));
    menu.append(Some(&gettext("Quit")), Some("win.quit"));
}

fn create_station_action(
    station: Station,
    play_button: &Button,
    window: &ApplicationWindow,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) -> SimpleAction {
    let radio = radio.clone();
    let meta = meta.clone();
    let win_clone = window.clone();
    let play = play_button.clone();

    make_action(station.name(), move || {
        radio.set_station(station);
        meta.set_station(station);
        if play.is_visible() {
            let _ = adw::prelude::WidgetExt::activate_action(
                &win_clone,
                "win.play",
                None::<&glib::Variant>,
            );
        }
    })
}

fn other_station(s: Station) -> Station {
    match s {
        Station::Jpop => Station::Kpop,
        Station::Kpop => Station::Jpop,
    }
}
