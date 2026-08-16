//! Adding and editing a server.
//!
//! Every field here is one the core needs to make a connection, and none of them is invented: the
//! sign-in methods are exactly the ones `TargetConfig::auth_kind` accepts, and the host key
//! policies exactly the ones the transport implements.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::glib;
use gtk4::Orientation;

use crate::store::SavedHost;

/// The sign-in methods, in the order the dialog offers them.
///
/// `key_text` — pasting the key body — is deliberately absent. It exists because a phone has no
/// user-visible filesystem to point at; a desktop has one, and an ssh-agent besides.
const AUTH_KINDS: [(&str, &str); 3] = [
    ("agent", "Use my SSH agent"),
    ("key", "A key file"),
    ("password", "A password"),
];

/// Host key policies, worst-last.
const POLICIES: [(&str, &str); 3] = [
    ("strict", "Only servers I already trust"),
    ("accept_new", "Trust on first connection"),
    ("accept_any", "Accept any key (unsafe)"),
];

/// What the dialog produces: the record to save, and a secret that must not be saved with it.
pub struct HostEdit {
    pub host: SavedHost,
    pub secret: Option<String>,
}

/// Open the add/edit sheet. `existing` is `None` when adding.
pub fn edit_host<F>(parent: &impl IsA<gtk4::Window>, existing: Option<SavedHost>, on_save: F)
where
    F: Fn(HostEdit) + 'static,
{
    let editing = existing.is_some();
    let host = existing.unwrap_or_else(|| SavedHost::new(""));

    let window = adw::Window::new();
    window.set_modal(true);
    window.set_transient_for(Some(parent.as_ref()));
    window.set_default_size(460, -1);
    window.set_title(Some(if editing {
        "Edit server"
    } else {
        "Add a server"
    }));

    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    header.set_show_start_title_buttons(false);

    let cancel = gtk4::Button::with_label("Cancel");
    let save = gtk4::Button::with_label(if editing { "Save" } else { "Add" });
    save.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&save);

    let page = adw::PreferencesPage::new();

    let where_group = adw::PreferencesGroup::new();
    where_group.set_title("Where");
    let address = adw::EntryRow::new();
    address.set_title("Address");
    address.set_text(&host.address);
    let user = adw::EntryRow::new();
    user.set_title("Username");
    user.set_text(&host.user);
    let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
    port.set_title("Port");
    port.set_value(host.port as f64);
    where_group.add(&address);
    where_group.add(&user);
    where_group.add(&port);
    page.add(&where_group);

    let sign_in = adw::PreferencesGroup::new();
    sign_in.set_title("Signing in");
    let auth = combo("Method", &AUTH_KINDS, &host.auth_kind);
    let key_path = adw::EntryRow::new();
    key_path.set_title("Key file");
    key_path.set_text(host.key_path.as_deref().unwrap_or(""));

    let browse = gtk4::Button::from_icon_name("document-open-symbolic");
    browse.set_valign(gtk4::Align::Center);
    browse.add_css_class("flat");
    key_path.add_suffix(&browse);

    let secret = adw::PasswordEntryRow::new();
    secret.set_title("Password");

    sign_in.add(&auth);
    sign_in.add(&key_path);
    sign_in.add(&secret);
    page.add(&sign_in);

    let note = gtk4::Label::new(Some(
        "A password is used for this session only and is never written to disk.",
    ));
    note.add_css_class("sg-tile-summary");
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_margin_start(12);
    note.set_margin_end(12);

    let safety = adw::PreferencesGroup::new();
    safety.set_title("Server identity");
    let policy = combo("Trust", &POLICIES, &host.host_key_policy);
    safety.add(&policy);
    page.add(&safety);

    let refresh_group = adw::PreferencesGroup::new();
    refresh_group.set_title("Refresh");
    let refresh = adw::SpinRow::with_range(250.0, 60_000.0, 250.0);
    refresh.set_title("Interval");
    refresh.set_subtitle("Milliseconds between readings");
    refresh.set_value(host.refresh_ms as f64);
    refresh_group.add(&refresh);
    page.add(&refresh_group);

    // Only the fields the chosen method actually uses. Showing a key path beside "use my agent"
    // invites someone to fill it in and then wonder why it was ignored.
    let sync_visibility = {
        let auth = auth.clone();
        let key_path = key_path.clone();
        let secret = secret.clone();
        let note = note.clone();
        move || {
            let kind = AUTH_KINDS[auth.selected() as usize].0;
            key_path.set_visible(kind == "key");
            secret.set_visible(kind == "password" || kind == "key");
            secret.set_title(if kind == "key" {
                "Key passphrase"
            } else {
                "Password"
            });
            note.set_visible(kind == "password" || kind == "key");
        }
    };
    sync_visibility();
    {
        let sync = sync_visibility.clone();
        auth.connect_selected_notify(move |_| sync());
    }

    {
        let key_path = key_path.clone();
        let window = window.clone();
        browse.connect_clicked(move |_| {
            let dialog = gtk4::FileDialog::new();
            dialog.set_title("Choose a private key");
            let key_path = key_path.clone();
            dialog.open(Some(&window), gtk4::gio::Cancellable::NONE, move |result| {
                // A cancelled picker is not a failure and must not be reported as one.
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        key_path.set_text(&path.to_string_lossy());
                    }
                }
            });
        });
    }

    let content = gtk4::Box::new(Orientation::Vertical, 0);
    content.append(&page);
    content.append(&note);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));

    let problem = Rc::new(RefCell::new(host));

    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }

    {
        let window = window.clone();
        let address = address.clone();
        let user = user.clone();
        let port = port.clone();
        let auth = auth.clone();
        let key_path = key_path.clone();
        let secret = secret.clone();
        let policy = policy.clone();
        let refresh = refresh.clone();
        let base = Rc::clone(&problem);

        save.connect_clicked(move |_| {
            let address_text = address.text().trim().to_string();
            if address_text.is_empty() {
                // Nothing can be done with a host that has no address, and saving one produces a
                // row in the sidebar that can never connect.
                address.add_css_class("error");
                return;
            }
            address.remove_css_class("error");

            let kind = AUTH_KINDS[auth.selected() as usize].0.to_string();
            let mut host = base.borrow().clone();
            host.address = address_text;
            host.user = user.text().trim().to_string();
            host.port = port.value() as u16;
            host.auth_kind = kind.clone();
            host.key_path = match kind.as_str() {
                "key" => Some(key_path.text().to_string()).filter(|p| !p.trim().is_empty()),
                _ => None,
            };
            host.host_key_policy = POLICIES[policy.selected() as usize].0.to_string();
            host.refresh_ms = refresh.value() as u64;

            let typed = secret.text().to_string();
            let secret_value = if kind == "agent" || typed.is_empty() {
                None
            } else {
                Some(typed)
            };

            on_save(HostEdit {
                host,
                secret: secret_value,
            });
            window.close();
        });
    }

    window.present();
}

/// Ask before doing something that cannot be undone.
pub fn confirm_removal<F>(parent: &impl IsA<gtk4::Window>, label: &str, on_confirm: F)
where
    F: Fn() + 'static,
{
    let dialog = adw::MessageDialog::new(
        Some(parent.as_ref()),
        Some("Remove this server?"),
        Some(&format!(
            "{label} will be removed from this device. Nothing on the server itself is changed."
        )),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("remove", "Remove");
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |dialog, response| {
        if response == "remove" {
            on_confirm();
        }
        dialog.close();
    });
    dialog.present();
}

/// Report something that went wrong, in the plainest words available.
pub fn report_problem(parent: &impl IsA<gtk4::Window>, title: &str, detail: &str) {
    let dialog = adw::MessageDialog::new(Some(parent.as_ref()), Some(title), Some(detail));
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.connect_response(None, |dialog, _| dialog.close());
    dialog.present();
}

fn combo(title: &str, options: &[(&str, &str); 3], selected: &str) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    let model = gtk4::StringList::new(&options.iter().map(|(_, label)| *label).collect::<Vec<_>>());
    row.set_model(Some(&model));
    let index = options
        .iter()
        .position(|(value, _)| *value == selected)
        .unwrap_or(0);
    row.set_selected(index as u32);
    row
}

/// Keep `glib` referenced for the closures above even when features change.
#[allow(dead_code)]
fn _glib_in_use() -> glib::Type {
    glib::Type::STRING
}
