// SPDX-License-Identifier: GPL-3.0-or-later
//! The main window: queue, empty state, sidebar with output settings, and the
//! bottom action bar. Engine work runs on worker threads (util::spawn_worker).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::card::Card;
use crate::i18n::{gettext, ntr_fmt, tr_fmt};
use crate::model::{self, Item, Status};
use crate::sidebar::Sidebar;
use crate::util::{self, spawn_worker};
use retort_core::branding;
use retort_core::engine::Engine;
use retort_core::hw::Inventory;
use retort_core::media::{analyze, MediaInfo};
use retort_core::plan::HwPrefs;
use retort_core::process::SystemRunner;
use retort_core::settings::OutputSettings;

/// Hardware probing status (shown in the header subtitle and preferences).
#[derive(Debug, Clone, PartialEq)]
pub enum HwStatus {
    Starting,
    Probing { done: usize, total: usize },
    Ready,
    Disabled,
}

pub struct State {
    pub engine: Option<Engine>,
    pub engine_error: Option<String>,
    pub hw: HwStatus,
    pub items: Vec<Item>,
    pub next_id: u64,
    /// Settings new files get (the sidebar edits these when nothing is selected).
    pub defaults: OutputSettings,
    pub preset: String,
    pub batch: bool,
    pub inhibit: Option<u32>,
    pub batch_done: u32,
    pub batch_failed: u32,
    pub batch_fallback: u32,
    pub last_output: Option<PathBuf>,
}

pub struct Win {
    pub app: adw::Application,
    pub window: adw::ApplicationWindow,
    pub settings: gio::Settings,
    pub toasts: adw::ToastOverlay,
    pub split: adw::OverlaySplitView,
    pub stack: gtk::Stack,
    pub list: gtk::ListBox,
    pub title: adw::WindowTitle,
    pub drop_overlay: gtk::Box,
    pub content_view: adw::ToolbarView,
    pub convert_btn: gtk::Button,
    pub clear_btn: gtk::Button,
    pub stop_btn: gtk::Button,
    pub pause_btn: gtk::Button,
    pub total_label: gtk::Label,
    pub total_bar: gtk::ProgressBar,
    pub sidebar: Sidebar,
    pub state: RefCell<State>,
    pub cards: RefCell<HashMap<u64, Card>>,
    /// Suppress sidebar → settings feedback while the sidebar is being filled.
    pub syncing: Cell<bool>,
    pub demo: bool,
    /// The hardware matrix container while Preferences is open.
    pub prefs_hw: RefCell<Option<gtk::Box>>,
    /// Phone-width layout (compact cards).
    pub compact: Cell<bool>,
    /// The running or last preview sample.
    pub preview: RefCell<Option<crate::preview::State>>,
}

pub type Ui = Rc<Win>;

impl Win {
    pub fn toast(&self, text: &str) {
        let t = adw::Toast::new(text);
        t.set_timeout(4);
        self.toasts.add_toast(t);
    }

    pub fn hw_prefs(&self) -> HwPrefs {
        let s = &self.settings;
        HwPrefs {
            disable_hardware: !s.boolean("use-hardware"),
            preferred_backend: retort_core::hw::Backend::from_key(&s.string("preferred-backend")),
            device: Some(s.string("preferred-device").to_string()).filter(|d| !d.is_empty()),
        }
    }

    /// Engine with the current hardware preferences applied.
    pub fn engine(&self) -> Option<Engine> {
        let st = self.state.borrow();
        st.engine.clone().map(|mut e| {
            e.prefs = self.hw_prefs();
            e
        })
    }

    pub fn item_index(&self, id: u64) -> Option<usize> {
        self.state.borrow().items.iter().position(|i| i.id == id)
    }

    pub fn update_card(&self, id: u64) {
        let st = self.state.borrow();
        if let (Some(item), Some(card)) = (
            st.items.iter().find(|i| i.id == id),
            self.cards.borrow().get(&id),
        ) {
            card.update(item);
        }
    }

    pub fn save_queue(&self) {
        if self.demo {
            return;
        }
        model::save(&self.state.borrow().items, &model::queue_file());
    }

    /// Selected item ids (in list order).
    pub fn selected_ids(&self) -> Vec<u64> {
        let rows = self.list.selected_rows();
        let cards = self.cards.borrow();
        let mut ids: Vec<(i32, u64)> = cards
            .iter()
            .filter(|(_, c)| rows.contains(&c.row))
            .map(|(id, c)| (c.row.index(), *id))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
}

/// Build and show the main window (or reuse the existing one), adding `files`.
pub fn present(app: &adw::Application, files: Vec<PathBuf>, demo: bool) -> Ui {
    if let Some(ui) = app_ui(app) {
        add_files(&ui, files);
        ui.window.present();
        return ui;
    }
    let ui = build(app, demo);
    unsafe {
        app.set_data("retort-ui", ui.clone());
    }
    ui.window.present();
    if !demo {
        start_engine(&ui);
        restore_queue(&ui);
        #[cfg(feature = "update-check")]
        crate::updates::maybe_check(&ui);
    }
    add_files(&ui, files);
    ui
}

/// The window state attached to the application, if created.
pub fn app_ui(app: &adw::Application) -> Option<Ui> {
    unsafe { app.data::<Ui>("retort-ui").map(|p| p.as_ref().clone()) }
}

fn build(app: &adw::Application, demo: bool) -> Ui {
    let settings = util::settings(demo);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(branding::APP_NAME)
        .default_width(settings.int("window-width"))
        .default_height(settings.int("window-height"))
        .width_request(360)
        .height_request(400)
        .build();
    if settings.boolean("window-maximized") && !demo {
        window.maximize();
    }

    // ---- Header bar.
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::new(branding::APP_NAME, "");
    header.set_title_widget(Some(&title));
    let add_menu = gio::Menu::new();
    add_menu.append(Some(&gettext("Add Files…")), Some("win.add-files"));
    add_menu.append(Some(&gettext("Add Folder…")), Some("win.add-folder"));
    let add_btn = adw::SplitButton::builder()
        .icon_name("list-add-symbolic")
        .action_name("win.add-files")
        .menu_model(&add_menu)
        .tooltip_text(gettext("Add Files"))
        .dropdown_tooltip(gettext("More Ways to Add"))
        .build();
    header.pack_start(&add_btn);

    let main_menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some(&gettext("Clear Finished")), Some("win.clear-finished"));
    s1.append(Some(&gettext("Clear Queue")), Some("win.clear-all"));
    main_menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some(&gettext("System Check")), Some("app.system-check"));
    s2.append(Some(&gettext("Preferences")), Some("app.preferences"));
    s2.append(Some(&gettext("Keyboard Shortcuts")), Some("app.shortcuts"));
    s2.append(
        Some(&tr_fmt("About {}", &[branding::APP_NAME])),
        Some("app.about"),
    );
    main_menu.append_section(None, &s2);
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&main_menu)
        .primary(true)
        .tooltip_text(gettext("Main Menu"))
        .build();
    header.pack_end(&menu_btn);
    let sidebar_btn = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text(gettext("Output Settings"))
        .active(true)
        .build();
    header.pack_end(&sidebar_btn);

    // ---- Empty state.
    let add_big = gtk::Button::with_label(&gettext("Add Files…"));
    add_big.add_css_class("pill");
    add_big.add_css_class("suggested-action");
    add_big.set_halign(gtk::Align::Center);
    add_big.set_action_name(Some("win.add-files"));
    let empty = adw::StatusPage::builder()
        .icon_name(format!("{}-symbolic", branding::APP_ID))
        .title(gettext("Drop Files to Convert"))
        .description(gettext("Video and audio files, or whole folders"))
        .child(&add_big)
        .vexpand(true)
        .build();
    let empty_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    empty_frame.add_css_class("drop-zone");
    empty_frame.append(&empty);

    // ---- Queue.
    let list = gtk::ListBox::new();
    list.add_css_class("queue");
    list.set_selection_mode(gtk::SelectionMode::Multiple);
    list.set_activate_on_single_click(false);
    list.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Conversion queue",
    ))]);
    let clamp = adw::Clamp::builder()
        .maximum_size(920)
        .tightening_threshold(600)
        .child(&list)
        .build();
    clamp.set_margin_start(12);
    clamp.set_margin_end(12);
    clamp.set_margin_top(6);
    clamp.set_margin_bottom(12);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&clamp)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.add_named(&empty_frame, Some("empty"));
    stack.add_named(&scroller, Some("queue"));

    // Drop highlight overlay.
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&stack));
    let drop_overlay = gtk::Box::new(gtk::Orientation::Vertical, 0);
    drop_overlay.add_css_class("drop-overlay");
    drop_overlay.set_can_target(false);
    drop_overlay.set_visible(false);
    overlay.add_overlay(&drop_overlay);

    // ---- Bottom bar.
    let total_label = gtk::Label::new(None);
    total_label.add_css_class("total-status");
    total_label.add_css_class("dim-label");
    total_label.set_xalign(0.0);
    total_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let total_bar = gtk::ProgressBar::new();
    total_bar.set_visible(false);
    total_bar.add_css_class("card-progress");
    let totals = gtk::Box::new(gtk::Orientation::Vertical, 4);
    totals.set_hexpand(true);
    totals.set_valign(gtk::Align::Center);
    totals.append(&total_label);
    totals.append(&total_bar);
    let pause_btn = util::icon_button("media-playback-pause-symbolic", &gettext("Pause All"));
    pause_btn.add_css_class("circular");
    pause_btn.set_visible(false);
    let stop_btn = gtk::Button::with_label(&gettext("Stop"));
    stop_btn.add_css_class("pill");
    stop_btn.add_css_class("destructive-action");
    stop_btn.set_visible(false);
    stop_btn.set_action_name(Some("win.stop"));
    let convert_btn = gtk::Button::with_label(&gettext("Convert"));
    convert_btn.add_css_class("pill");
    convert_btn.add_css_class("suggested-action");
    convert_btn.set_action_name(Some("win.convert"));
    convert_btn.set_tooltip_text(Some(&gettext("Convert all files (Ctrl+Enter)")));
    let clear_btn = gtk::Button::with_label(&gettext("Clear Finished"));
    clear_btn.add_css_class("pill");
    clear_btn.set_action_name(Some("win.clear-finished"));
    clear_btn.set_visible(false);
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bar.add_css_class("action-bar");
    bar.append(&totals);
    bar.append(&pause_btn);
    bar.append(&stop_btn);
    bar.append(&clear_btn);
    bar.append(&convert_btn);

    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&header);
    content_view.set_content(Some(&overlay));
    content_view.add_bottom_bar(&bar);
    content_view.set_bottom_bar_style(adw::ToolbarStyle::Raised);

    // ---- Sidebar.
    let sidebar = Sidebar::build();

    let split = adw::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .sidebar(&sidebar.root)
        .content(&content_view)
        .min_sidebar_width(320.0)
        .max_sidebar_width(400.0)
        .sidebar_width_fraction(0.36)
        .show_sidebar(true)
        .build();
    split
        .bind_property("show-sidebar", &sidebar_btn, "active")
        .bidirectional()
        .sync_create()
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&split));
    window.set_content(Some(&toasts));

    let bp = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 760sp").expect("valid condition"),
    );
    bp.add_setter(&split, "collapsed", Some(&true.to_value()));
    bp.add_setter(&split, "show-sidebar", Some(&false.to_value()));
    window.add_breakpoint(bp);
    // Phone width: also compact cards. The last matching breakpoint wins, so
    // it repeats the setters above.
    let narrow = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 520sp").expect("valid condition"),
    );
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    narrow.add_setter(&split, "show-sidebar", Some(&false.to_value()));
    window.add_breakpoint(narrow.clone());

    let defaults = retort_core::preset::find(&settings.string("preset"))
        .or_else(|| retort_core::preset::find("mp4-compatible"))
        .map(|p| p.settings)
        .expect("builtin preset");
    let ui: Ui = Rc::new(Win {
        app: app.clone(),
        window,
        settings,
        toasts,
        split,
        stack,
        list,
        title,
        drop_overlay,
        content_view,
        convert_btn,
        clear_btn,
        stop_btn,
        pause_btn,
        total_label,
        total_bar,
        sidebar,
        state: RefCell::new(State {
            engine: None,
            engine_error: None,
            hw: HwStatus::Starting,
            items: Vec::new(),
            next_id: 1,
            defaults,
            preset: String::new(),
            batch: false,
            inhibit: None,
            batch_done: 0,
            batch_failed: 0,
            batch_fallback: 0,
            last_output: None,
        }),
        cards: RefCell::default(),
        syncing: Cell::new(false),
        demo,
        prefs_hw: RefCell::new(None),
        compact: Cell::new(false),
        preview: RefCell::new(None),
    });
    for (signal_compact, connect) in [(true, true), (false, false)] {
        let u = ui.clone();
        let f = move |_: &adw::Breakpoint| set_compact(&u, signal_compact);
        if connect {
            narrow.connect_apply(f);
        } else {
            narrow.connect_unapply(f);
        }
    }
    {
        let mut st = ui.state.borrow_mut();
        st.preset = ui.settings.string("preset").to_string();
        let mb = ui.settings.double("target-size-mb");
        st.defaults = crate::sidebar::apply_speed_and_size(
            st.defaults.clone(),
            &st.preset,
            &ui.settings.string("speed"),
            mb,
        );
    }
    wire(&ui);
    crate::sidebar::wire(&ui);
    crate::sidebar::sync_from_state(&ui);
    refresh(&ui);
    ui
}

fn wire(ui: &Ui) {
    // Window actions.
    let add = |name: &str, f: Box<dyn Fn()>| {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| f());
        ui.window.add_action(&a);
    };
    let u = ui.clone();
    add("add-files", Box::new(move || choose_files(&u)));
    let u = ui.clone();
    add("add-folder", Box::new(move || choose_folder(&u)));
    let u = ui.clone();
    add("convert", Box::new(move || crate::runner::start_batch(&u)));
    let u = ui.clone();
    add("stop", Box::new(move || crate::runner::stop_all(&u)));
    let u = ui.clone();
    add(
        "remove-selected",
        Box::new(move || {
            for id in u.selected_ids() {
                remove_item(&u, id);
            }
        }),
    );
    let u = ui.clone();
    add(
        "clear-finished",
        Box::new(move || {
            let ids: Vec<u64> = u
                .state
                .borrow()
                .items
                .iter()
                .filter(|i| i.status.is_finished())
                .map(|i| i.id)
                .collect();
            for id in ids {
                remove_item(&u, id);
            }
        }),
    );
    let u = ui.clone();
    add(
        "clear-all",
        Box::new(move || {
            let ids: Vec<u64> = u
                .state
                .borrow()
                .items
                .iter()
                .filter(|i| !i.status.is_active())
                .map(|i| i.id)
                .collect();
            for id in ids {
                remove_item(&u, id);
            }
        }),
    );
    let u = ui.clone();
    add("select-all", Box::new(move || u.list.select_all()));
    let u = ui.clone();
    ui.pause_btn
        .connect_clicked(move |_| crate::runner::toggle_pause_all(&u));

    // Delete removes selected files, but only while the queue has focus
    // (an application accelerator would steal Delete from text fields).
    let keys = gtk::EventControllerKey::new();
    let u = ui.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Delete || key == gdk::Key::KP_Delete {
            for id in u.selected_ids() {
                remove_item(&u, id);
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    ui.list.add_controller(keys);

    // Selection → sidebar scope.
    let u = ui.clone();
    ui.list
        .connect_selected_rows_changed(move |_| crate::sidebar::sync_from_state(&u));
    let u = ui.clone();
    ui.list.connect_row_activated(move |_, row| {
        let id = u
            .cards
            .borrow()
            .iter()
            .find(|(_, c)| &c.row == row)
            .map(|(id, _)| *id);
        if let Some(id) = id {
            crate::details::show(&u, id);
        }
    });

    // Drag and drop of files and folders.
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let u = ui.clone();
    drop.connect_enter(move |_, _, _| {
        u.drop_overlay.set_visible(true);
        gdk::DragAction::COPY
    });
    let u = ui.clone();
    drop.connect_leave(move |_| u.drop_overlay.set_visible(false));
    let u = ui.clone();
    drop.connect_drop(move |_, value, _, _| {
        u.drop_overlay.set_visible(false);
        let Ok(list) = value.get::<gdk::FileList>() else {
            return false;
        };
        let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
        add_files(&u, paths);
        true
    });
    ui.window.add_controller(drop);

    // Persist window size.
    let u = ui.clone();
    ui.window.connect_close_request(move |w| {
        if !u.demo {
            let (wd, ht) = w.default_size();
            let _ = u.settings.set_int("window-width", wd);
            let _ = u.settings.set_int("window-height", ht);
            let _ = u.settings.set_boolean("window-maximized", w.is_maximized());
            u.save_queue();
        }
        if u.state.borrow().batch {
            crate::runner::stop_all(&u);
        }
        crate::preview::cleanup(&u);
        glib::Propagation::Proceed
    });
}

/// Update empty state, title, bottom bar and action sensitivity.
pub fn refresh(ui: &Ui) {
    let st = ui.state.borrow();
    let n = st.items.len();
    ui.stack
        .set_visible_child_name(if n == 0 { "empty" } else { "queue" });
    let total_size: u64 = st
        .items
        .iter()
        .filter_map(|i| i.media.as_ref().and_then(|m| m.size))
        .sum();
    let subtitle = if n == 0 {
        String::new()
    } else {
        ntr_fmt(
            "{} file · {}",
            "{} files · {}",
            n as u32,
            &[&n.to_string(), &util::human_size(total_size)],
        )
    };
    ui.title.set_subtitle(&subtitle);
    let convertible = st
        .items
        .iter()
        .filter(|i| i.status.is_convertible())
        .count();
    let active = st.items.iter().filter(|i| i.status.is_active()).count();
    let can_convert = convertible > 0 && st.engine.is_some() && !st.batch;
    if let Some(a) = ui
        .window
        .lookup_action("convert")
        .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
    {
        a.set_enabled(can_convert);
    }
    let done_n = st
        .items
        .iter()
        .filter(|i| matches!(i.status, Status::Done { .. }))
        .count();
    let failed_n = st
        .items
        .iter()
        .filter(|i| matches!(i.status, Status::Failed { .. } | Status::Unreadable { .. }))
        .count();
    let finished = st.items.iter().filter(|i| i.status.is_finished()).count();
    // Nothing left to convert: offer to clear the finished files instead.
    let all_finished = !st.batch && convertible == 0 && finished > 0;
    ui.convert_btn.set_visible(!st.batch && !all_finished);
    ui.clear_btn.set_visible(all_finished);
    ui.content_view.set_reveal_bottom_bars(n > 0);
    ui.stop_btn.set_visible(st.batch);
    ui.pause_btn.set_visible(st.batch);
    let any_running = st.items.iter().any(|i| i.status == Status::Running);
    let any_paused = st.items.iter().any(|i| i.status == Status::Paused);
    ui.pause_btn.set_icon_name(if any_paused && !any_running {
        "media-playback-start-symbolic"
    } else {
        "media-playback-pause-symbolic"
    });
    let label = if st.engine_error.is_some() {
        gettext("FFmpeg not found — open System Check")
    } else if st.engine.is_none() {
        gettext("Starting…")
    } else if st.batch {
        let fr: Vec<f64> = st
            .items
            .iter()
            .filter(|i| i.status.is_active() || matches!(i.status, Status::Done { .. }))
            .map(|i| match i.status {
                Status::Done { .. } => 1.0,
                _ => i.live.fraction.unwrap_or(0.0),
            })
            .collect();
        let frac = if fr.is_empty() {
            0.0
        } else {
            fr.iter().sum::<f64>() / fr.len() as f64
        };
        ui.total_bar.set_fraction(frac);
        let done = st.batch_done;
        let total = done + active as u32;
        let eta = st
            .items
            .iter()
            .filter_map(|i| i.live.eta)
            .fold(0.0f64, f64::max);
        let mut s = tr_fmt("{} of {} done", &[&done.to_string(), &total.to_string()]);
        if eta > 0.0 {
            s.push_str(" · ");
            s.push_str(&tr_fmt("about {} left", &[&util::eta(eta)]));
        }
        s
    } else if n == 0 {
        String::new()
    } else {
        match &st.hw {
            HwStatus::Probing { done, total } => tr_fmt(
                "Testing hardware acceleration… {}/{}",
                &[&done.to_string(), &total.to_string()],
            ),
            _ if convertible > 0 => ntr_fmt(
                "{} file ready",
                "{} files ready",
                convertible as u32,
                &[&convertible.to_string()],
            ),
            _ if failed_n > 0 => tr_fmt(
                "{} converted, {} failed",
                &[&done_n.to_string(), &failed_n.to_string()],
            ),
            _ if done_n > 0 => ntr_fmt(
                "{} file converted",
                "{} files converted",
                done_n as u32,
                &[&done_n.to_string()],
            ),
            _ => String::new(),
        }
    };
    ui.total_label.set_text(&label);
    ui.total_bar.set_visible(st.batch);
    drop(st);
}

/// Analysis result: media info, PNG thumbnail, waveform peaks.
type Analysis = (MediaInfo, Option<Vec<u8>>, Option<Vec<f32>>);

/// Locate ffmpeg and load capabilities, then probe hardware (cached).
pub fn start_engine(ui: &Ui) {
    let preferred = Some(ui.settings.string("ffmpeg-path").to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let u = ui.clone();
    spawn_worker(
        move || {
            Engine::discover(preferred.as_deref(), SystemRunner::shared())
                .map_err(|e| e.to_string())
        },
        move |res| {
            match res {
                Ok(engine) => {
                    {
                        let mut st = u.state.borrow_mut();
                        st.engine = Some(engine);
                        st.engine_error = None;
                    }
                    analyze_pending(&u);
                    probe_hardware(&u, false);
                }
                Err(e) => {
                    u.state.borrow_mut().engine_error = Some(e.clone());
                    let toast = adw::Toast::builder()
                        .title(gettext("FFmpeg was not found"))
                        .button_label(gettext("System Check"))
                        .action_name("app.system-check")
                        .timeout(0)
                        .build();
                    u.toasts.add_toast(toast);
                }
            }
            refresh(&u);
            crate::sidebar::sync_from_state(&u);
        },
    );
}

/// Probe hardware (or load the cache). `force` re-tests.
pub fn probe_hardware(ui: &Ui, force: bool) {
    let Some(engine) = ui.state.borrow().engine.clone() else {
        return;
    };
    if !ui.settings.boolean("use-hardware") && !force {
        ui.state.borrow_mut().hw = HwStatus::Disabled;
        return;
    }
    ui.state.borrow_mut().hw = HwStatus::Probing { done: 0, total: 0 };
    refresh(ui);
    let (ptx, prx) = async_channel::unbounded::<(usize, usize)>();
    let u = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok((d, t)) = prx.recv().await {
            if matches!(u.state.borrow().hw, HwStatus::Probing { .. }) {
                u.state.borrow_mut().hw = HwStatus::Probing { done: d, total: t };
                refresh(&u);
            }
        }
    });
    let u = ui.clone();
    spawn_worker(
        move || {
            let cb = move |d: usize, t: usize| {
                let _ = ptx.send_blocking((d, t));
            };
            engine.probe_hardware(force, Some(&cb))
        },
        move |inv: Inventory| set_inventory(&u, inv, force),
    );
}

pub fn set_inventory(ui: &Ui, inv: Inventory, announce: bool) {
    let summary: Vec<String> = inv
        .device_summaries()
        .into_iter()
        .filter(|d| !d.encode.is_empty())
        .map(|d| {
            format!(
                "{} ({})",
                retort_core::plan::short_device_name(&d.label),
                d.encode
                    .iter()
                    .map(|b| b.label())
                    .collect::<Vec<_>>()
                    .join("/")
            )
        })
        .collect();
    {
        let mut st = ui.state.borrow_mut();
        if let Some(e) = st.engine.take() {
            st.engine = Some(e.with_inventory(inv));
        }
        st.hw = HwStatus::Ready;
    }
    if announce {
        ui.toast(&if summary.is_empty() {
            gettext("No working hardware encoder found — using the CPU")
        } else {
            tr_fmt("Hardware acceleration: {}", &[&summary.join(", ")])
        });
    }
    replan_all(ui);
    refresh(ui);
    crate::sidebar::sync_from_state(ui);
    crate::prefs::refresh_hardware(ui);
}

fn restore_queue(ui: &Ui) {
    for (path, settings, preset, status) in model::load(&model::queue_file()) {
        let id = {
            let mut st = ui.state.borrow_mut();
            let id = st.next_id;
            st.next_id += 1;
            let mut item = Item::new(id, path, settings, preset);
            item.status = match status {
                Status::Ready => Status::Analyzing,
                s => s,
            };
            st.items.push(item);
            id
        };
        add_card(ui, id);
    }
    refresh(ui);
}

/// Add files (folders are expanded). Duplicates of queued paths are ignored.
pub fn add_files(ui: &Ui, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let exts = Some(ui.settings.string("import-extensions").to_string())
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| model::DEFAULT_EXTENSIONS.to_string());
    let expanded = model::expand_inputs(paths, &exts);
    let mut added = 0;
    for path in expanded {
        let id = {
            let mut st = ui.state.borrow_mut();
            if st
                .items
                .iter()
                .any(|i| i.path == path && !i.status.is_finished())
            {
                continue;
            }
            let id = st.next_id;
            st.next_id += 1;
            let item = Item::new(id, path, st.defaults.clone(), st.preset.clone());
            st.items.push(item);
            id
        };
        add_card(ui, id);
        added += 1;
    }
    if added == 0 {
        ui.toast(&gettext("No new media files found"));
    }
    analyze_pending(ui);
    refresh(ui);
    ui.save_queue();
}

/// Insert a fully-formed item (screenshot harness).
pub fn insert_item(ui: &Ui, item: Item) {
    let id = item.id;
    {
        let mut st = ui.state.borrow_mut();
        st.next_id = st.next_id.max(id + 1);
        st.items.push(item);
    }
    add_card(ui, id);
}

fn set_compact(ui: &Ui, compact: bool) {
    ui.compact.set(compact);
    let ids: Vec<u64> = ui.cards.borrow().keys().copied().collect();
    for id in ids {
        if let Some(c) = ui.cards.borrow().get(&id) {
            c.set_compact(compact);
        }
        ui.update_card(id);
    }
}

fn add_card(ui: &Ui, id: u64) {
    let card = {
        let st = ui.state.borrow();
        let Some(item) = st.items.iter().find(|i| i.id == id) else {
            return;
        };
        let card = Card::new(item);
        if ui.compact.get() {
            card.set_compact(true);
            card.update(item);
        }
        card
    };
    ui.list.append(&card.row);
    let u = ui.clone();
    card.connect_remove(move || remove_item(&u, id));
    let u = ui.clone();
    card.connect_cancel(move || crate::runner::cancel_item(&u, id));
    let u = ui.clone();
    card.connect_pause(move || crate::runner::toggle_pause_item(&u, id));
    let u = ui.clone();
    card.add_action("details", move || crate::details::show(&u, id));
    let u = ui.clone();
    card.add_action("show", move || {
        let out = match &u
            .state
            .borrow()
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.status.clone())
        {
            Some(Status::Done { output, .. }) => Some(output.clone()),
            _ => None,
        };
        if let Some(o) = out {
            util::show_in_files(Some(u.window.upcast_ref()), &o);
        }
    });
    let u = ui.clone();
    card.add_action("copy-command", move || {
        let cmd = u
            .state
            .borrow()
            .items
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| i.command.clone());
        if let Some(c) = cmd {
            util::copy_text(&u.window, &c);
            u.toast(&gettext("Command copied"));
        }
    });
    let u = ui.clone();
    card.add_action("copy-diagnostics", move || {
        let text = crate::details::diagnostics_text(&u, id);
        util::copy_text(&u.window, &text);
        u.toast(&gettext(
            "Diagnostics copied — paste them into a bug report",
        ));
    });
    let u = ui.clone();
    card.add_action("retry-software", move || {
        crate::runner::retry_software(&u, id)
    });
    ui.cards.borrow_mut().insert(id, card);
    ui.update_card(id);
}

pub fn remove_item(ui: &Ui, id: u64) {
    let Some(idx) = ui.item_index(id) else { return };
    if ui.state.borrow().items[idx].status.is_active() {
        return;
    }
    ui.state.borrow_mut().items.remove(idx);
    // Release the borrow before touching the list: removing a selected row
    // emits selected-rows-changed synchronously, and its handler reads
    // `cards` (a held borrow_mut panicked there and aborted the app).
    let card = ui.cards.borrow_mut().remove(&id);
    if let Some(card) = card {
        ui.list.remove(&card.row);
    }
    refresh(ui);
    crate::sidebar::sync_from_state(ui);
    ui.save_queue();
}

/// Analyse every item still in `Analyzing` (needs the engine).
pub fn analyze_pending(ui: &Ui) {
    let Some(engine) = ui.state.borrow().engine.clone() else {
        return;
    };
    let todo: Vec<(u64, PathBuf)> = ui
        .state
        .borrow()
        .items
        .iter()
        .filter(|i| i.status == Status::Analyzing && i.media.is_none())
        .map(|i| (i.id, i.path.clone()))
        .collect();
    for (id, path) in todo {
        let e = engine.clone();
        let u = ui.clone();
        spawn_worker(
            move || {
                let media = e.analyze(&path).map_err(|e| e.to_string())?;
                let can_tonemap = e.caps.has_filter("zscale");
                let thumb = if media.has_video() || media.streams.iter().any(|s| s.attached_pic) {
                    analyze::thumbnail_png(&e.ff, &media, 256, can_tonemap).ok()
                } else {
                    None
                };
                let peaks = if thumb.is_none() && media.primary_audio().is_some() {
                    analyze::waveform_peaks(&e.ff, &media, 48).ok()
                } else {
                    None
                };
                Ok::<_, String>((media, thumb, peaks))
            },
            move |res| {
                let finished_analysis = |u: &Ui, m: Result<Analysis, String>| {
                    let Some(idx) = u.item_index(id) else { return };
                    let mut st = u.state.borrow_mut();
                    let item = &mut st.items[idx];
                    match m {
                        Ok((media, thumb, peaks)) => {
                            item.media = Some(media);
                            item.thumbnail = thumb.and_then(|b| {
                                gdk::Texture::from_bytes(&glib::Bytes::from(&b)).ok()
                            });
                            item.peaks = peaks;
                            if item.status == Status::Analyzing {
                                item.status = Status::Ready;
                            }
                        }
                        Err(e) => {
                            item.status = Status::Unreadable {
                                reason: tr_fmt("Can't read this file: {}", &[&e]),
                            };
                        }
                    }
                };
                finished_analysis(&u, res);
                replan(&u, id);
                refresh(&u);
                crate::sidebar::sync_from_state(&u);
            },
        );
    }
}

/// Recompute the predicted plan (badge, notes) of one item.
pub fn replan(ui: &Ui, id: u64) {
    let Some(engine) = ui.engine() else { return };
    let (media, settings) = {
        let st = ui.state.borrow();
        let Some(item) = st.items.iter().find(|i| i.id == id) else {
            return;
        };
        if item.status.is_active() {
            return;
        }
        let Some(m) = item.media.clone() else { return };
        (m, item.settings.clone())
    };
    let u = ui.clone();
    spawn_worker(
        move || engine.plan(&media, &settings).map_err(|e| e.to_string()),
        move |res| {
            if let Some(idx) = u.item_index(id) {
                let mut st = u.state.borrow_mut();
                let item = &mut st.items[idx];
                if item.status.is_active() {
                    return;
                }
                match res {
                    Ok(plan) => {
                        let first = &plan.chain[0];
                        item.summary = Some(first.summary.clone());
                        item.badge = Some(first.badge.clone());
                        item.hardware = first.hardware;
                        item.notes = plan.notes.clone();
                        if matches!(item.status, Status::Skipped { .. }) {
                            item.status = Status::Ready;
                        }
                    }
                    Err(e) => {
                        item.summary = None;
                        item.badge = None;
                        item.notes = vec![e];
                    }
                }
            }
            u.update_card(id);
            crate::sidebar::update_plan_preview(&u);
        },
    );
}

pub fn replan_all(ui: &Ui) {
    let ids: Vec<u64> = ui
        .state
        .borrow()
        .items
        .iter()
        .filter(|i| i.media.is_some())
        .map(|i| i.id)
        .collect();
    for id in ids {
        replan(ui, id);
    }
}

fn media_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.set_name(Some(&gettext("Video and Audio")));
    f.add_mime_type("video/*");
    f.add_mime_type("audio/*");
    f.add_mime_type("image/gif");
    f.add_mime_type("image/webp");
    f
}

fn choose_files(ui: &Ui) {
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&media_filter());
    let all = gtk::FileFilter::new();
    all.set_name(Some(&gettext("All Files")));
    all.add_pattern("*");
    filters.append(&all);
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Add Files"))
        .modal(true)
        .filters(&filters)
        .build();
    let u = ui.clone();
    dialog.open_multiple(Some(&ui.window), gio::Cancellable::NONE, move |res| {
        if let Ok(model) = res {
            let paths: Vec<PathBuf> = (0..model.n_items())
                .filter_map(|i| model.item(i).and_downcast::<gio::File>())
                .filter_map(|f| f.path())
                .collect();
            add_files(&u, paths);
        }
    });
}

fn choose_folder(ui: &Ui) {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Add Folder"))
        .modal(true)
        .build();
    let u = ui.clone();
    dialog.select_multiple_folders(Some(&ui.window), gio::Cancellable::NONE, move |res| {
        if let Ok(model) = res {
            let paths: Vec<PathBuf> = (0..model.n_items())
                .filter_map(|i| model.item(i).and_downcast::<gio::File>())
                .filter_map(|f| f.path())
                .collect();
            add_files(&u, paths);
        }
    });
}
