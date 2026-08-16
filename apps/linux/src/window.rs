//! The window: a list of servers on the left, one server on the right.
//!
//! The split is the desktop shape of the same navigation the other platforms use — a sidebar on a
//! Mac, a stack on a phone, two panes on a foldable. What differs per platform is only what should.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4::glib;
use gtk4::Orientation;

use sg_ffi::ConnectionState;

use crate::command::CommandView;
use crate::dialogs;
use crate::engine::Engine;
use crate::store::{self, Paths, SavedHost};
use crate::views::{ProcessTable, SimpleView, TechnicalView};
use crate::widgets::set_level_class;

/// How often the interface asks the core for the newest completed refresh.
///
/// The core ticks on each host's own interval and publishes a finished snapshot; this is only the
/// display timer reading it. Twice a second is faster than the quickest refresh anyone configures,
/// so a reading never sits on screen longer than it had to.
const DISPLAY_INTERVAL: Duration = Duration::from_millis(500);

pub struct Window {
    window: adw::ApplicationWindow,
    engine: Engine,
    paths: Paths,
    hosts: Rc<RefCell<Vec<SavedHost>>>,
    /// Passwords and passphrases, for this run of the app only.
    ///
    /// Never written anywhere. Linux has no equivalent of the Keychain that ServerGlass can use
    /// without pulling in a Secret Service dependency it cannot test on a headless machine, so
    /// rather than pretend to remember a password it says plainly that it does not.
    secrets: Rc<RefCell<HashMap<String, String>>>,
    selected: Rc<RefCell<Option<String>>>,
    list: gtk4::ListBox,
    rows: RefCell<HashMap<String, (adw::ActionRow, gtk4::Image)>>,
    title: gtk4::Label,
    subtitle: gtk4::Label,
    simple: SimpleView,
    technical: TechnicalView,
    processes: ProcessTable,
    command: Rc<CommandView>,
    content: gtk4::Stack,
}

impl Window {
    pub fn new(app: &adw::Application, paths: Paths) -> Rc<Window> {
        let engine = Engine::new(&paths);
        let loaded = store::load(&paths);
        let hosts = Rc::new(RefCell::new(loaded.clone().unwrap_or_default()));

        let window = adw::ApplicationWindow::new(app);
        window.set_title(Some("ServerGlass"));
        window.set_default_size(1080, 720);

        // Sidebar.
        let list = gtk4::ListBox::new();
        list.set_selection_mode(gtk4::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");

        let list_scroll = gtk4::ScrolledWindow::new();
        list_scroll.set_vexpand(true);
        list_scroll.set_child(Some(&list));

        let add = gtk4::Button::from_icon_name("list-add-symbolic");
        add.set_tooltip_text(Some("Add a server"));
        let pair = gtk4::Button::from_icon_name("send-to-symbolic");
        pair.set_tooltip_text(Some("Pair with another device"));

        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&gtk4::Label::new(Some("Servers"))));
        sidebar_header.pack_start(&add);
        sidebar_header.pack_end(&pair);

        let sidebar_toolbar = adw::ToolbarView::new();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        sidebar_toolbar.set_content(Some(&list_scroll));
        let sidebar_page = adw::NavigationPage::new(&sidebar_toolbar, "Servers");

        // Detail.
        let simple = SimpleView::new();
        let technical = TechnicalView::new();
        let processes = ProcessTable::new();
        let command = CommandView::new(engine.clone());

        let stack = adw::ViewStack::new();
        stack.add_titled_with_icon(
            &simple.widget(),
            Some("simple"),
            "Summary",
            "view-grid-symbolic",
        );
        stack.add_titled_with_icon(
            &technical.widget(),
            Some("technical"),
            "Every reading",
            "view-list-symbolic",
        );
        stack.add_titled_with_icon(
            &processes.widget(),
            Some("processes"),
            "Processes",
            "view-list-ordered-symbolic",
        );
        stack.add_titled_with_icon(
            command.widget(),
            Some("command"),
            "Command",
            "utilities-terminal-symbolic",
        );

        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);

        let title = gtk4::Label::new(None);
        title.add_css_class("title");
        let subtitle = gtk4::Label::new(None);
        subtitle.add_css_class("sg-dense");
        let titles = gtk4::Box::new(Orientation::Vertical, 0);
        titles.append(&title);
        titles.append(&subtitle);

        let edit = gtk4::Button::from_icon_name("document-edit-symbolic");
        edit.set_tooltip_text(Some("Edit this server"));
        let remove = gtk4::Button::from_icon_name("user-trash-symbolic");
        remove.set_tooltip_text(Some("Remove this server"));

        let detail_header = adw::HeaderBar::new();
        detail_header.set_title_widget(Some(&titles));
        detail_header.pack_end(&remove);
        detail_header.pack_end(&edit);

        let empty = adw::StatusPage::new();
        empty.set_icon_name(Some("network-server-symbolic"));
        empty.set_title("No servers yet");
        empty.set_description(Some(
            "Add a server you can already reach over SSH. Nothing is installed on it.",
        ));
        let empty_add = gtk4::Button::with_label("Add a server");
        empty_add.add_css_class("suggested-action");
        empty_add.add_css_class("pill");
        empty_add.set_halign(gtk4::Align::Center);
        empty.set_child(Some(&empty_add));

        // One or the other: the switcher is meaningless with nothing selected.
        let content = gtk4::Stack::new();
        content.add_named(&empty, Some("empty"));
        content.add_named(&stack, Some("host"));

        let detail_toolbar = adw::ToolbarView::new();
        detail_toolbar.add_top_bar(&detail_header);
        detail_toolbar.set_content(Some(&content));

        let bottom = adw::ViewSwitcherBar::new();
        bottom.set_stack(Some(&stack));
        detail_toolbar.add_bottom_bar(&bottom);

        let detail_page = adw::NavigationPage::new(&detail_toolbar, "Server");

        let split = adw::NavigationSplitView::new();
        split.set_sidebar(Some(&sidebar_page));
        split.set_content(Some(&detail_page));
        split.set_min_sidebar_width(240.0);
        window.set_content(Some(&split));

        let this = Rc::new(Window {
            window,
            engine,
            paths,
            hosts,
            secrets: Rc::new(RefCell::new(HashMap::new())),
            selected: Rc::new(RefCell::new(None)),
            list,
            rows: RefCell::new(HashMap::new()),
            title,
            subtitle,
            simple,
            technical,
            processes,
            command,
            content,
        });

        // A damaged inventory is reported once, on screen, rather than silently starting empty —
        // which is indistinguishable from the app having forgotten every server.
        if let Err(problem) = loaded {
            let window = this.window.clone();
            let problem = problem.clone();
            glib::idle_add_local_once(move || {
                dialogs::report_problem(
                    &window,
                    "Your server list could not be read",
                    &format!("{problem}\n\nNothing has been overwritten. Adding a server now would replace the file."),
                );
            });
        }

        this.connect(&add, &pair, &edit, &remove, &empty_add);
        this.rebuild_list();
        this.start_display_timer();
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn connect(
        self: &Rc<Self>,
        add: &gtk4::Button,
        pair: &gtk4::Button,
        edit: &gtk4::Button,
        remove: &gtk4::Button,
        empty_add: &gtk4::Button,
    ) {
        {
            let this = Rc::clone(self);
            self.list.connect_row_selected(move |_, row| {
                let Some(row) = row else { return };
                let id = row.widget_name().to_string();
                this.select(&id);
            });
        }

        for button in [add, empty_add] {
            let this = Rc::clone(self);
            button.connect_clicked(move |_| this.add_host());
        }

        {
            let this = Rc::clone(self);
            edit.connect_clicked(move |_| this.edit_selected());
        }

        {
            let this = Rc::clone(self);
            remove.connect_clicked(move |_| this.remove_selected());
        }

        {
            let this = Rc::clone(self);
            pair.connect_clicked(move |_| {
                crate::pairing::open(&this.window, &this.paths, Rc::clone(&this.hosts), {
                    let this = Rc::clone(&this);
                    move || {
                        this.persist();
                        this.rebuild_list();
                    }
                });
            });
        }
    }

    /// Rebuild the sidebar from the inventory.
    fn rebuild_list(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.rows.borrow_mut().clear();

        for host in self.hosts.borrow().iter() {
            let row = adw::ActionRow::new();
            row.set_title(&glib::markup_escape_text(&host.label()));
            row.set_subtitle(&format!("port {}", host.port));
            row.set_widget_name(&host.id);

            let dot = gtk4::Image::from_icon_name("media-record-symbolic");
            set_level_class(&dot, "checking");
            row.add_prefix(&dot);

            self.list.append(&row);
            self.rows.borrow_mut().insert(host.id.clone(), (row, dot));
        }

        let empty = self.hosts.borrow().is_empty();
        self.content
            .set_visible_child_name(if empty { "empty" } else { "host" });
        if empty {
            *self.selected.borrow_mut() = None;
            self.command.set_host(None);
            self.title.set_text("");
            self.subtitle.set_text("");
            return;
        }

        // Keep the current selection if it still exists, otherwise take the first host.
        let keep = self.selected.borrow().clone();
        let target = keep
            .filter(|id| self.hosts.borrow().iter().any(|h| &h.id == id))
            .or_else(|| self.hosts.borrow().first().map(|h| h.id.clone()));
        if let Some(id) = target {
            if let Some((row, _)) = self.rows.borrow().get(&id) {
                self.list.select_row(Some(row));
            }
            self.select(&id);
        }
    }

    fn select(self: &Rc<Self>, host_id: &str) {
        *self.selected.borrow_mut() = Some(host_id.to_string());
        self.content.set_visible_child_name("host");

        let Some(host) = self.host(host_id) else {
            return;
        };
        self.title.set_text(&host.label());
        self.command.set_host(Some(host.id.clone()));
        self.ensure_started(&host);
        self.refresh();
    }

    /// Connect a host, asking for its secret first if it needs one and none is held.
    fn ensure_started(self: &Rc<Self>, host: &SavedHost) {
        if self.engine.is_running(&host.id) {
            return;
        }
        let needs_secret = matches!(host.auth_kind.as_str(), "password" | "key");
        let held = self.secrets.borrow().get(&host.id).cloned();

        if needs_secret && held.is_none() && host.auth_kind == "password" {
            // A key may well have no passphrase, so only a password is asked for up front.
            self.ask_for_secret(host.clone());
            return;
        }
        if let Err(problem) = self.engine.start(host, held) {
            dialogs::report_problem(&self.window, "This server could not be started", &problem);
        }
    }

    fn ask_for_secret(self: &Rc<Self>, host: SavedHost) {
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some("Password needed"),
            Some(&format!(
                "Enter the password for {}. It is used for this session only and is never written \
                 to disk.",
                host.label()
            )),
        );
        let entry = gtk4::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        entry.set_margin_top(8);
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("connect", "Connect");
        dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("connect"));
        dialog.set_close_response("cancel");

        let this = Rc::clone(self);
        dialog.connect_response(None, move |dialog, response| {
            if response == "connect" {
                let secret = entry.text().to_string();
                let secret = (!secret.is_empty()).then_some(secret);
                if let Some(value) = &secret {
                    this.secrets
                        .borrow_mut()
                        .insert(host.id.clone(), value.clone());
                }
                if let Err(problem) = this.engine.start(&host, secret) {
                    dialogs::report_problem(
                        &this.window,
                        "This server could not be started",
                        &problem,
                    );
                }
            }
            dialog.close();
        });
        dialog.present();
    }

    fn add_host(self: &Rc<Self>) {
        let this = Rc::clone(self);
        dialogs::edit_host(&self.window, None, move |edit| {
            if let Some(secret) = edit.secret {
                this.secrets
                    .borrow_mut()
                    .insert(edit.host.id.clone(), secret);
            }
            let id = edit.host.id.clone();
            this.hosts.borrow_mut().push(edit.host);
            this.persist();
            *this.selected.borrow_mut() = Some(id);
            this.rebuild_list();
        });
    }

    fn edit_selected(self: &Rc<Self>) {
        let Some(host) = self.selected_host() else {
            return;
        };
        let this = Rc::clone(self);
        dialogs::edit_host(&self.window, Some(host.clone()), move |edit| {
            if let Some(secret) = edit.secret {
                this.secrets
                    .borrow_mut()
                    .insert(edit.host.id.clone(), secret);
            }
            {
                let mut hosts = this.hosts.borrow_mut();
                if let Some(slot) = hosts.iter_mut().find(|h| h.id == edit.host.id) {
                    *slot = edit.host.clone();
                }
            }
            this.persist();
            // The address or the sign-in method may have changed, so the connection is rebuilt
            // rather than left pointed at the old one.
            let secret = this.secrets.borrow().get(&edit.host.id).cloned();
            if let Err(problem) = this.engine.restart(&edit.host, secret) {
                dialogs::report_problem(&this.window, "This server could not be started", &problem);
            }
            this.rebuild_list();
        });
    }

    fn remove_selected(self: &Rc<Self>) {
        let Some(host) = self.selected_host() else {
            return;
        };
        let this = Rc::clone(self);
        dialogs::confirm_removal(&self.window, &host.label(), move || {
            this.engine.forget(&host.id);
            this.secrets.borrow_mut().remove(&host.id);
            this.hosts.borrow_mut().retain(|h| h.id != host.id);
            *this.selected.borrow_mut() = None;
            this.persist();
            this.rebuild_list();
        });
    }

    fn persist(self: &Rc<Self>) {
        if let Err(problem) = store::save(&self.paths, &self.hosts.borrow()) {
            // Losing the inventory silently is how somebody finds their servers gone next launch.
            dialogs::report_problem(
                &self.window,
                "Your server list could not be saved",
                &problem,
            );
        }
    }

    fn host(&self, host_id: &str) -> Option<SavedHost> {
        self.hosts
            .borrow()
            .iter()
            .find(|h| h.id == host_id)
            .cloned()
    }

    fn selected_host(&self) -> Option<SavedHost> {
        let id = self.selected.borrow().clone()?;
        self.host(&id)
    }

    fn start_display_timer(self: &Rc<Self>) {
        let this = Rc::clone(self);
        glib::timeout_add_local(DISPLAY_INTERVAL, move || {
            this.refresh();
            glib::ControlFlow::Continue
        });
    }

    /// Read the newest snapshot for every host and render the selected one.
    fn refresh(&self) {
        for host in self.hosts.borrow().iter() {
            let Some(snapshot) = self.engine.snapshot(&host.id) else {
                continue;
            };
            if let Some((row, dot)) = self.rows.borrow().get(&host.id) {
                set_level_class(dot, &snapshot.health.level);
                let name = if snapshot.display_name.is_empty() {
                    host.label()
                } else {
                    snapshot.display_name.clone()
                };
                row.set_title(&glib::markup_escape_text(&name));
                row.set_subtitle(&glib::markup_escape_text(&snapshot.health.headline));
            }
        }

        let Some(id) = self.selected.borrow().clone() else {
            return;
        };
        let Some(snapshot) = self.engine.snapshot(&id) else {
            return;
        };

        let name = if snapshot.display_name.is_empty() {
            self.host(&id).map(|h| h.label()).unwrap_or_default()
        } else {
            snapshot.display_name.clone()
        };
        self.title.set_text(&name);
        self.subtitle.set_text(&describe(&snapshot.state));
        set_level_class(&self.subtitle, &snapshot.health.level);

        self.simple.render(&snapshot);
        self.technical.render(&snapshot);
        self.processes.render(&snapshot);
    }
}

/// The connection state, in words rather than a variant name.
///
/// Only the state is worded here; every sentence about a *reading* comes from `sg-ffi::plain`.
fn describe(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Idle => "Not connected".into(),
        ConnectionState::Connecting => "Connecting…".into(),
        ConnectionState::Online => "Connected".into(),
        ConnectionState::Reconnecting { retry_in_ms, .. } => {
            format!("Reconnecting in {}s", (retry_in_ms / 1000).max(1))
        }
        ConnectionState::Failed { .. } => "Not reachable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reconnect_countdown_never_reads_zero_seconds() {
        // A "reconnecting in 0s" that sits there for a second reads as a stuck app.
        let state = ConnectionState::Reconnecting {
            attempt: 1,
            retry_in_ms: 400,
        };
        assert_eq!(describe(&state), "Reconnecting in 1s");
    }

    #[test]
    fn every_connection_state_has_words() {
        for state in [
            ConnectionState::Idle,
            ConnectionState::Connecting,
            ConnectionState::Online,
            ConnectionState::Reconnecting {
                attempt: 2,
                retry_in_ms: 4000,
            },
            ConnectionState::Failed {
                message: "no route to host".into(),
                recoverable: true,
            },
        ] {
            assert!(!describe(&state).is_empty());
        }
    }

    #[test]
    fn a_failure_is_not_dressed_up_in_the_header() {
        // The header says only that it is not reachable; the sentence explaining *why* comes from
        // the core, which knows which failures it can honestly explain.
        let state = ConnectionState::Failed {
            message: "authentication failed".into(),
            recoverable: false,
        };
        assert_eq!(describe(&state), "Not reachable");
    }
}
