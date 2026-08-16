//! The command runner.
//!
//! **Not a terminal.** No PTY is allocated, so anything interactive — `top`, `vim`, a `sudo`
//! password prompt — produces nothing useful or hangs until the core's sixty-second limit. That is
//! the honest shape of the transport underneath, and the screen says so rather than letting
//! someone discover it by watching a command hang.
//!
//! This is also the one place ServerGlass sends anything to a host that is not a read: the *user*
//! types the command. Invariant 1 is about the app never installing or modifying anything of its
//! own accord, and that is unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Entry, Label, Orientation, ScrolledWindow, TextView};

use crate::engine::Engine;

pub struct CommandView {
    root: gtk4::Box,
    entry: Entry,
    run: gtk4::Button,
    output: TextView,
    status: Label,
    host_id: Rc<RefCell<Option<String>>>,
}

impl CommandView {
    pub fn new(engine: Engine) -> Rc<CommandView> {
        let root = gtk4::Box::new(Orientation::Vertical, 12);
        root.set_margin_top(20);
        root.set_margin_bottom(20);
        root.set_margin_start(20);
        root.set_margin_end(20);

        let caution = Label::new(Some(
            "Runs one command over the connection the readings already use. \
             Interactive programs such as top or vim cannot run here.",
        ));
        caution.add_css_class("sg-tile-summary");
        caution.set_wrap(true);
        caution.set_xalign(0.0);
        root.append(&caution);

        let line = gtk4::Box::new(Orientation::Horizontal, 8);
        let entry = Entry::new();
        entry.set_placeholder_text(Some("systemctl status nginx"));
        entry.set_hexpand(true);
        let run = gtk4::Button::with_label("Run");
        run.add_css_class("suggested-action");
        line.append(&entry);
        line.append(&run);
        root.append(&line);

        let status = Label::new(None);
        status.add_css_class("sg-dense");
        status.set_xalign(0.0);
        status.set_visible(false);
        root.append(&status);

        let output = TextView::new();
        output.set_editable(false);
        output.set_monospace(true);
        output.add_css_class("sg-output");
        output.set_wrap_mode(gtk4::WrapMode::WordChar);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&output));
        scroll.set_valign(Align::Fill);
        root.append(&scroll);

        let view = Rc::new(CommandView {
            root,
            entry,
            run,
            output,
            status,
            host_id: Rc::new(RefCell::new(None)),
        });

        let clicked = Rc::clone(&view);
        let engine_for_click = engine.clone();
        view.run.connect_clicked(move |_| {
            clicked.submit(&engine_for_click);
        });

        let activated = Rc::clone(&view);
        view.entry.connect_activate(move |_| {
            activated.submit(&engine);
        });

        view
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }

    /// Point the runner at a host, clearing whatever the last one printed.
    ///
    /// Output is not carried across a change of host: a wall of text from a different machine, with
    /// nothing on screen saying so, is how somebody reads an answer about the wrong server.
    pub fn set_host(&self, host_id: Option<String>) {
        let changed = *self.host_id.borrow() != host_id;
        if changed {
            self.output.buffer().set_text("");
            self.status.set_visible(false);
        }
        *self.host_id.borrow_mut() = host_id;
    }

    fn submit(self: &Rc<Self>, engine: &Engine) {
        let command = self.entry.text().trim().to_string();
        if command.is_empty() {
            return;
        }
        let Some(host_id) = self.host_id.borrow().clone() else {
            return;
        };
        let Some(target_id) = engine.target_id(&host_id) else {
            self.report(
                "This server is not connected, so the command was not run.",
                true,
            );
            return;
        };

        self.run.set_sensitive(false);
        self.entry.set_sensitive(false);
        self.report("Running…", false);

        let core = engine.core();
        let view = Rc::clone(self);
        let echo = command.clone();

        glib::spawn_future_local(async move {
            // `run_command` blocks until the host answers. On the thread driving the interface that
            // would freeze the window for as long as the command takes, which for the sixty-second
            // ceiling means a minute of an unresponsive app.
            let answer = gio::spawn_blocking(move || core.run_command(target_id, command)).await;

            view.run.set_sensitive(true);
            view.entry.set_sensitive(true);

            match answer {
                Ok(Ok(result)) => {
                    let buffer = view.output.buffer();
                    let existing = buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), false)
                        .to_string();
                    let separator = if existing.is_empty() { "" } else { "\n" };
                    buffer.set_text(&format!("{existing}{separator}$ {echo}\n{}", result.output));
                    view.report(
                        &format!("exit {} · {} ms", result.exit_code, result.elapsed_ms),
                        result.exit_code != 0,
                    );
                }
                Ok(Err(error)) => view.report(&error.to_string(), true),
                // The worker itself failed rather than the command. Saying "the command failed"
                // here would be a confident, wrong explanation.
                Err(_) => view.report("The command could not be started.", true),
            }
        });
    }

    fn report(&self, message: &str, bad: bool) {
        self.status.set_visible(true);
        self.status.set_text(message);
        crate::widgets::set_level_class(&self.status, if bad { "problem" } else { "ok" });
    }
}
