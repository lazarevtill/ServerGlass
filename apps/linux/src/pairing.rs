//! Moving an inventory between two devices.
//!
//! This is not a sync service: there is no server, no account, and nothing persists between the
//! two devices afterwards. One device shows a code, the other reads it, both derive the same six
//! digits from the full transcript, and a person compares two screens before anything is sent.
//!
//! The whole security argument rests on that comparison happening *before* the transfer, which is
//! why the flow below stops and waits for a human in the middle rather than completing in one
//! call. Every byte of the handshake, the sealing and the merge rules lives in `sg-sync`; this
//! file moves text between that state machine and the screen.
//!
//! **No credential crosses.** Passwords, passphrases and pasted keys are not fields of the wire
//! format at all — `crates/sg-sync` has a test asserting the exact set of fields, so adding one
//! fails on purpose. The receiving device asks for each secret itself.

use std::cell::RefCell;
use std::net::{IpAddr, UdpSocket};
use std::rc::Rc;
use std::sync::mpsc;

use adw::prelude::*;
use gtk4::glib;
use gtk4::Orientation;

use sg_sync::{
    accept_transfer, merge, send_transfer, transfer::write_payload, Listener, Offer, Payload,
    SyncHost,
};

use crate::dialogs;
use crate::store::{self, Paths, SavedHost};

/// What the worker tells the screen.
enum Event {
    /// The offer to show as a QR, when this device is the one being paired *to*.
    Offer(String),
    /// The six digits both devices must show.
    Code(String),
    /// An inventory arrived.
    Received(Payload),
    /// The inventory was sent.
    Sent,
    Failed(String),
}

/// What the screen tells the worker: whether the codes matched.
type Confirmation = bool;

/// Open the pairing chooser.
pub fn open<F>(
    parent: &adw::ApplicationWindow,
    paths: &Paths,
    hosts: Rc<RefCell<Vec<SavedHost>>>,
    on_change: F,
) where
    F: Fn() + Clone + 'static,
{
    let dialog = adw::MessageDialog::new(
        Some(parent),
        Some("Pair with another device"),
        Some(
            "Servers and the identities you have trusted move across. Passwords and keys never do \
             — the other device asks for those itself.",
        ),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("receive", "Receive servers");
    dialog.add_response("send", "Send my servers");
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    let parent = parent.clone();
    let paths = paths.clone();
    dialog.connect_response(None, move |dialog, response| {
        dialog.close();
        match response {
            "receive" => receive(&parent, &paths, Rc::clone(&hosts), on_change.clone()),
            "send" => send(&parent, &paths, Rc::clone(&hosts)),
            _ => {}
        }
    });
    dialog.present();
}

/// This device shows a code and waits for an inventory.
fn receive<F>(
    parent: &adw::ApplicationWindow,
    paths: &Paths,
    hosts: Rc<RefCell<Vec<SavedHost>>>,
    on_change: F,
) where
    F: Fn() + 'static,
{
    let (to_ui, from_worker) = async_channel::unbounded::<Event>();
    let (to_worker, from_ui) = mpsc::channel::<Confirmation>();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                return;
            }
        };
        runtime.block_on(async move {
            let addresses = local_addresses();
            let listener = match Listener::bind(&addresses).await {
                Ok(listener) => listener,
                Err(e) => {
                    let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                    return;
                }
            };
            if to_ui
                .send_blocking(Event::Offer(listener.offer().encode()))
                .is_err()
            {
                return;
            }

            let (session, mut stream) = match listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                    return;
                }
            };
            if to_ui
                .send_blocking(Event::Code(session.verification_code()))
                .is_err()
            {
                return;
            }

            // Nothing is read off the wire until a person has said the two codes match.
            match from_ui.recv() {
                Ok(true) => {}
                _ => return,
            }

            match accept_transfer(&session, &mut stream).await {
                Ok(bytes) => match Payload::from_json(&bytes) {
                    Ok(payload) => {
                        let _ = to_ui.send_blocking(Event::Received(payload));
                    }
                    Err(e) => {
                        let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                }
            }
        });
    });

    let window = sheet(parent, "Receive servers");
    let body = window.1.clone();
    let window = window.0;

    let waiting = gtk4::Label::new(Some("Finding this device's address…"));
    waiting.set_wrap(true);
    body.append(&waiting);

    let paths = paths.clone();
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        while let Ok(event) = from_worker.recv().await {
            match event {
                Event::Offer(text) => {
                    waiting
                        .set_text("Scan this on the other device, or copy the text below into it.");
                    body.append(&qr_widget(&text));
                    body.append(&copyable(&text));
                }
                Event::Code(code) => {
                    let confirm = to_worker.clone();
                    let shown = window.clone();
                    let closing = window.clone();
                    compare_codes(&shown, &code, move |matched| {
                        let _ = confirm.send(matched);
                        if !matched {
                            // They differ. Something is intercepting; nothing is read or sent.
                            closing.close();
                        }
                    });
                }
                Event::Received(payload) => {
                    window.close();
                    apply(&parent, &paths, &hosts, payload);
                    on_change();
                    return;
                }
                Event::Failed(problem) => {
                    window.close();
                    dialogs::report_problem(&parent, "Pairing did not finish", &problem);
                    return;
                }
                Event::Sent => return,
            }
        }
    });
}

/// This device reads the other device's code and sends its inventory.
fn send(parent: &adw::ApplicationWindow, paths: &Paths, hosts: Rc<RefCell<Vec<SavedHost>>>) {
    let (window, body) = sheet(parent, "Send my servers");

    let explain = gtk4::Label::new(Some(
        "On the other device, choose “Receive servers”, then copy the code it shows into here.",
    ));
    explain.set_wrap(true);
    explain.set_xalign(0.0);
    body.append(&explain);

    let entry = gtk4::TextView::new();
    entry.set_monospace(true);
    entry.set_wrap_mode(gtk4::WrapMode::WordChar);
    entry.add_css_class("sg-output");
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_min_content_height(120);
    scroll.set_child(Some(&entry));
    body.append(&scroll);

    let go = gtk4::Button::with_label("Connect");
    go.add_css_class("suggested-action");
    go.set_halign(gtk4::Align::End);
    body.append(&go);

    let payload = inventory(paths, &hosts);
    let parent = parent.clone();
    let window_for_click = window.clone();
    let body_for_click = body.clone();

    go.connect_clicked(move |go| {
        let buffer = entry.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_string();
        if text.is_empty() {
            return;
        }
        let offer = match Offer::decode(&text) {
            Ok(offer) => offer,
            Err(e) => {
                dialogs::report_problem(&parent, "That code could not be read", &e.to_string());
                return;
            }
        };
        go.set_sensitive(false);

        let (to_ui, from_worker) = async_channel::unbounded::<Event>();
        let (to_worker, from_ui) = mpsc::channel::<Confirmation>();
        let payload = payload.clone();

        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let (session, mut stream) = match send_transfer(&offer).await {
                    Ok(pair) => pair,
                    Err(e) => {
                        let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                        return;
                    }
                };
                if to_ui
                    .send_blocking(Event::Code(session.verification_code()))
                    .is_err()
                {
                    return;
                }
                match from_ui.recv() {
                    Ok(true) => {}
                    _ => return,
                }

                let bytes = match payload.to_json() {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                        return;
                    }
                };
                match write_payload(&session, &mut stream, &bytes).await {
                    Ok(()) => {
                        let _ = to_ui.send_blocking(Event::Sent);
                    }
                    Err(e) => {
                        let _ = to_ui.send_blocking(Event::Failed(e.to_string()));
                    }
                }
            });
        });

        let parent = parent.clone();
        let window = window_for_click.clone();
        let body = body_for_click.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = from_worker.recv().await {
                match event {
                    Event::Code(code) => {
                        let confirm = to_worker.clone();
                        let shown = window.clone();
                        let closing = window.clone();
                        compare_codes(&shown, &code, move |matched| {
                            let _ = confirm.send(matched);
                            if !matched {
                                // They differ. Something is intercepting; nothing is read or sent.
                                closing.close();
                            }
                        });
                    }
                    Event::Sent => {
                        let done = gtk4::Label::new(Some("Sent."));
                        done.add_css_class("sg-ok");
                        body.append(&done);
                        return;
                    }
                    Event::Failed(problem) => {
                        window.close();
                        dialogs::report_problem(&parent, "Pairing did not finish", &problem);
                        return;
                    }
                    _ => {}
                }
            }
        });
    });

    window.present();
}

/// Ask whether the two screens show the same six digits.
fn compare_codes<F>(parent: &adw::Window, code: &str, answered: F)
where
    F: Fn(bool) + 'static,
{
    let dialog = adw::MessageDialog::new(
        Some(parent),
        Some("Do both devices show this?"),
        Some(
            "If the two codes are different, something is intercepting the connection. Stop, and \
             do not continue.",
        ),
    );
    let label = gtk4::Label::new(Some(code));
    label.add_css_class("sg-headline");
    label.add_css_class("sg-number");
    label.set_margin_top(8);
    dialog.set_extra_child(Some(&label));

    dialog.add_response("no", "They differ");
    dialog.add_response("yes", "They match");
    dialog.set_response_appearance("no", adw::ResponseAppearance::Destructive);
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("no");
    dialog.connect_response(None, move |dialog, response| {
        answered(response == "yes");
        dialog.close();
    });
    dialog.present();
}

/// Merge what arrived into what this device already had, and report what changed.
fn apply(
    parent: &adw::ApplicationWindow,
    paths: &Paths,
    hosts: &Rc<RefCell<Vec<SavedHost>>>,
    incoming: Payload,
) {
    let existing = inventory(paths, hosts);
    let result = merge(&existing, &incoming);

    // New hosts only. A host already here keeps the settings chosen on *this* device — a transfer
    // is not a reason to overwrite a deliberate local choice.
    {
        let mut hosts = hosts.borrow_mut();
        for host in &result.hosts {
            let already = hosts
                .iter()
                .any(|h| h.address == host.address && h.port == host.port && h.user == host.user);
            if already {
                continue;
            }
            let mut saved = SavedHost::new(&host.address);
            saved.port = host.port;
            saved.user = host.user.clone();
            saved.auth_kind = host.auth_kind.clone();
            saved.key_path = host.key_path.clone();
            saved.host_key_policy = host.host_key_policy.clone();
            saved.refresh_ms = host.refresh_ms;
            hosts.push(saved);
        }
    }

    let mut problems = Vec::new();
    if let Err(e) = store::save(paths, &hosts.borrow()) {
        problems.push(e);
    }
    if let Err(e) = write_known_hosts(paths, &result.known_hosts) {
        problems.push(e);
    }

    let mut summary = format!(
        "{} server(s) added, {} already here. {} trusted identities added.",
        result.added_hosts, result.kept_hosts, result.added_pins
    );
    if !result.conflicts.is_empty() {
        // A conflicting pin is never applied. A sync channel that can quietly rewrite one is a
        // machine-in-the-middle with extra steps.
        summary.push_str("\n\nThese identities disagree and were left exactly as they were:");
        for conflict in &result.conflicts {
            summary.push_str(&format!("\n· {}", conflict.host));
        }
        summary.push_str(
            "\n\nThat can mean the server was rebuilt — or that something is impersonating it. \
             Find out which before trusting the new one.",
        );
    }
    for problem in problems {
        summary.push_str(&format!("\n\n{problem}"));
    }

    dialogs::report_problem(parent, "Pairing finished", &summary);
}

/// This device's inventory in the wire shape.
fn inventory(paths: &Paths, hosts: &Rc<RefCell<Vec<SavedHost>>>) -> Payload {
    Payload {
        hosts: hosts
            .borrow()
            .iter()
            .map(|h| SyncHost {
                address: h.address.clone(),
                port: h.port,
                user: h.user.clone(),
                auth_kind: h.auth_kind.clone(),
                key_path: h.key_path.clone(),
                host_key_policy: h.host_key_policy.clone(),
                refresh_ms: h.refresh_ms,
            })
            .collect(),
        known_hosts: read_known_hosts(paths),
    }
}

fn read_known_hosts(paths: &Paths) -> Vec<String> {
    std::fs::read_to_string(&paths.known_hosts)
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn write_known_hosts(paths: &Paths, lines: &[String]) -> Result<(), String> {
    if let Some(dir) = paths.known_hosts.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&paths.known_hosts, text)
        .map_err(|e| format!("Could not write {}: {e}", paths.known_hosts.display()))
}

/// Every address this device might be reachable at.
///
/// A device usually has several, and which one reaches depends on where the other device is: over
/// WireGuard or Tailscale the tunnel address is often the only one that works. Asking the kernel
/// which source address it would use *toward each kind of destination* enumerates them without a
/// libc dependency — connecting a UDP socket sends nothing, it only resolves the route.
fn local_addresses() -> Vec<String> {
    // A public address for the default route, the CGNAT range Tailscale uses, and the two private
    // ranges a LAN or a WireGuard tunnel is usually numbered from.
    const PROBES: [&str; 4] = ["1.1.1.1:9", "100.64.0.1:9", "10.0.0.1:9", "192.168.1.1:9"];

    let mut found: Vec<String> = Vec::new();
    for probe in PROBES {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if socket.connect(probe).is_err() {
            continue;
        }
        let Ok(address) = socket.local_addr() else {
            continue;
        };
        let ip = address.ip();
        if ip.is_unspecified() || ip == IpAddr::from([127, 0, 0, 1]) {
            continue;
        }
        let text = ip.to_string();
        if !found.contains(&text) {
            found.push(text);
        }
    }
    found
}

/// Render an offer as a QR code.
fn qr_widget(text: &str) -> gtk4::DrawingArea {
    let area = gtk4::DrawingArea::new();
    area.set_content_width(260);
    area.set_content_height(260);
    area.set_halign(gtk4::Align::Center);

    let code = qrcode::QrCode::new(text.as_bytes()).ok();
    area.set_draw_func(move |_, cr, width, height| {
        let Some(code) = &code else { return };
        let modules = code.to_colors();
        let side = code.width();
        let size = (width.min(height)) as f64;
        let scale = size / (side + 2) as f64;

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.rectangle(0.0, 0.0, size, size);
        cr.fill().expect("cairo qr background");

        cr.set_source_rgb(0.0, 0.0, 0.0);
        for (index, colour) in modules.iter().enumerate() {
            if *colour == qrcode::Color::Dark {
                let x = (index % side) as f64;
                let y = (index / side) as f64;
                cr.rectangle((x + 1.0) * scale, (y + 1.0) * scale, scale, scale);
            }
        }
        cr.fill().expect("cairo qr modules");
    });
    area
}

/// The same text, selectable, for a device with no camera pointed at the screen.
fn copyable(text: &str) -> gtk4::ScrolledWindow {
    let view = gtk4::TextView::new();
    view.set_editable(false);
    view.set_monospace(true);
    view.set_wrap_mode(gtk4::WrapMode::Char);
    view.add_css_class("sg-output");
    view.buffer().set_text(text);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_min_content_height(90);
    scroll.set_child(Some(&view));
    scroll
}

/// A plain sheet with a header and a vertical body.
fn sheet(parent: &adw::ApplicationWindow, title: &str) -> (adw::Window, gtk4::Box) {
    let window = adw::Window::new();
    window.set_modal(true);
    window.set_transient_for(Some(parent));
    window.set_default_size(420, 560);
    window.set_title(Some(title));

    let body = gtk4::Box::new(Orientation::Vertical, 14);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    window.set_content(Some(&toolbar));
    window.present();
    (window, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_inventory_carries_no_credential() {
        // `SyncHost` has no field for one, and this fails the moment somebody adds it. The same
        // guarantee sg-sync asserts for the wire format, restated where the payload is built.
        let hosts = Rc::new(RefCell::new(vec![SavedHost::new("10.0.0.4")]));
        let dir = tempfile::tempdir().unwrap();
        let payload = inventory(&Paths::under(dir.path()), &hosts);

        let json = String::from_utf8(payload.to_json().unwrap()).unwrap();
        for forbidden in ["password", "passphrase", "secret", "key_text"] {
            assert!(
                !json.contains(forbidden),
                "the pairing payload mentioned {forbidden}"
            );
        }
    }

    #[test]
    fn known_hosts_survive_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let lines = vec![
            "[10.0.0.9]:2222 ssh-ed25519 AAAAC3Nz".to_string(),
            "10.0.0.4 ssh-ed25519 AAAAC3Nzb".to_string(),
        ];

        write_known_hosts(&paths, &lines).unwrap();
        assert_eq!(read_known_hosts(&paths), lines);
    }

    #[test]
    fn comments_and_blank_lines_are_not_offered_as_pins() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        std::fs::write(
            &paths.known_hosts,
            "# a comment\n\n10.0.0.4 ssh-ed25519 AAAA\n",
        )
        .unwrap();

        assert_eq!(read_known_hosts(&paths), vec!["10.0.0.4 ssh-ed25519 AAAA"]);
    }

    #[test]
    fn a_missing_known_hosts_file_offers_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_known_hosts(&Paths::under(dir.path())).is_empty());
    }
}
