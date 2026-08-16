//! The screens.
//!
//! Each view is built once and updated in place. A view rebuilds its children only when the
//! *shape* of what it is showing changes — a host gaining a swap partition, a container starting —
//! because tearing the tree down on every refresh would reset the scroll position under the reader
//! and drop focus out of whatever they were typing in.
//!
//! Nothing here decides what a reading means. Levels, wording, ordering and formatting all arrive
//! from `sg-ffi`; these functions choose widgets and put them in rows.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk4::prelude::*;
use gtk4::{Align, Label, Orientation, ScrolledWindow};

use sg_ffi::{format_value, TargetSnapshot};

use crate::widgets::{set_level_class, Bar, GaugeRow, Ring, Spark};

/// Drop every child of a box.
fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn section(title: &str) -> (gtk4::Box, gtk4::Box) {
    let outer = gtk4::Box::new(Orientation::Vertical, 8);
    let heading = Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    outer.append(&heading);

    let body = gtk4::Box::new(Orientation::Vertical, 8);
    outer.append(&body);
    (outer, body)
}

fn scroller(child: &impl IsA<gtk4::Widget>) -> ScrolledWindow {
    let scroll = ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(child));
    scroll
}

/// The default screen: a verdict, three readings, and nothing that needs explaining.
///
/// Deliberately three tiles and not four. Uptime used to be the fourth, but the health card's own
/// sentence already reads "Running for 13h 52m" — the tile repeated it, had no ring because uptime
/// is not a proportion of anything, and left the grid unbalanced.
pub struct SimpleView {
    root: gtk4::Box,
    headline: Label,
    detail: Label,
    tiles: gtk4::Box,
    built: RefCell<Vec<String>>,
    rings: RefCell<HashMap<String, Ring>>,
    numbers: RefCell<HashMap<String, (Label, Label, Spark)>>,
}

impl SimpleView {
    pub fn new() -> SimpleView {
        let root = gtk4::Box::new(Orientation::Vertical, 18);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);

        let card = gtk4::Box::new(Orientation::Vertical, 6);
        card.add_css_class("sg-card");

        let headline = Label::new(None);
        headline.add_css_class("sg-headline");
        headline.set_xalign(0.0);
        headline.set_wrap(true);

        let detail = Label::new(None);
        detail.add_css_class("sg-tile-summary");
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        // Two lines reserved whether or not both are needed, so "Barely working" and a full
        // sentence about free space produce cards of the same height and the tiles below them do
        // not jump on every refresh.
        detail.set_lines(2);
        detail.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        card.append(&headline);
        card.append(&detail);
        root.append(&card);

        // A horizontal box rather than a wrapping grid: the three headline tiles are always one
        // row. An adaptive grid wrapped them as 2 + 1 and left a hole beside the last one.
        let tiles = gtk4::Box::new(Orientation::Horizontal, 14);
        tiles.set_homogeneous(true);
        root.append(&tiles);

        SimpleView {
            root,
            headline,
            detail,
            tiles,
            built: RefCell::new(Vec::new()),
            rings: RefCell::new(HashMap::new()),
            numbers: RefCell::new(HashMap::new()),
        }
    }

    pub fn widget(&self) -> ScrolledWindow {
        scroller(&self.root)
    }

    pub fn render(&self, snapshot: &TargetSnapshot) {
        self.headline.set_text(&snapshot.health.headline);
        set_level_class(&self.headline, &snapshot.health.level);
        self.detail.set_text(&snapshot.health.detail);

        let signature: Vec<String> = snapshot
            .simple_tiles
            .iter()
            .map(|t| format!("{}:{}", t.metric, t.fraction.is_some()))
            .collect();

        if *self.built.borrow() != signature {
            clear(&self.tiles);
            self.rings.borrow_mut().clear();
            self.numbers.borrow_mut().clear();

            for tile in &snapshot.simple_tiles {
                let column = gtk4::Box::new(Orientation::Vertical, 8);
                column.add_css_class("sg-card");

                if tile.fraction.is_some() {
                    let ring = Ring::new(132);
                    column.append(ring.widget());
                    self.rings.borrow_mut().insert(tile.metric.clone(), ring);
                } else {
                    // No proportion, so no ring: a number and its trend instead.
                    let value = Label::new(None);
                    value.add_css_class("sg-tile-value");
                    value.set_xalign(0.0);
                    let name = Label::new(None);
                    name.add_css_class("sg-tile-name");
                    name.set_xalign(0.0);
                    let spark = Spark::new(40);

                    column.append(&value);
                    column.append(&name);
                    column.append(spark.widget());
                    self.numbers
                        .borrow_mut()
                        .insert(tile.metric.clone(), (value, name, spark));
                }

                let summary = Label::new(None);
                summary.add_css_class("sg-tile-summary");
                summary.set_wrap(true);
                summary.set_lines(2);
                summary.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                summary.set_xalign(0.0);
                summary.set_valign(Align::Start);
                summary.set_widget_name(&format!("summary-{}", tile.metric));
                column.append(&summary);

                self.tiles.append(&column);
            }
            *self.built.borrow_mut() = signature;
        }

        for tile in &snapshot.simple_tiles {
            if let Some(ring) = self.rings.borrow().get(&tile.metric) {
                ring.set(
                    tile.fraction.unwrap_or(0.0),
                    &tile.level,
                    &tile.value_text,
                    &tile.name,
                );
            }
            if let Some((value, name, spark)) = self.numbers.borrow().get(&tile.metric) {
                value.set_text(&tile.value_text);
                set_level_class(value, &tile.level);
                name.set_text(&tile.name);
                spark.set(&tile.history, &tile.level);
            }
            // The summary label is found by name rather than stored twice.
            if let Some(label) = find_named(&self.tiles, &format!("summary-{}", tile.metric)) {
                label.set_text(&tile.summary);
            }
        }
    }
}

impl Default for SimpleView {
    fn default() -> Self {
        SimpleView::new()
    }
}

/// Depth-first search for a label by widget name.
fn find_named(root: &impl IsA<gtk4::Widget>, name: &str) -> Option<Label> {
    let mut child = root.as_ref().first_child();
    while let Some(widget) = child {
        if widget.widget_name() == name {
            if let Ok(label) = widget.clone().downcast::<Label>() {
                return Some(label);
            }
        }
        if let Some(found) = find_named(&widget, name) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// Every reading, grouped the way the core grouped them.
///
/// Deliberately dense — small type, tight spacing, monospaced numbers so columns align and a
/// changing value does not make the layout twitch. That density is right for someone triaging a
/// server and wrong as a default, which is why the plain screen is the one that opens.
pub struct TechnicalView {
    root: gtk4::Box,
    facts: Label,
    errors: Label,
    headline: gtk4::Box,
    groups: gtk4::Box,
    entities: gtk4::Box,
    built: RefCell<String>,
    rows: RefCell<HashMap<String, GaugeRow>>,
}

impl TechnicalView {
    pub fn new() -> TechnicalView {
        let root = gtk4::Box::new(Orientation::Vertical, 18);
        root.set_margin_top(20);
        root.set_margin_bottom(20);
        root.set_margin_start(20);
        root.set_margin_end(20);

        let facts = Label::new(None);
        facts.add_css_class("sg-dense");
        facts.set_xalign(0.0);
        facts.set_wrap(true);
        root.append(&facts);

        let errors = Label::new(None);
        errors.add_css_class("sg-dense");
        errors.add_css_class("sg-problem");
        errors.set_xalign(0.0);
        errors.set_wrap(true);
        errors.set_visible(false);
        root.append(&errors);

        let (headline_section, headline) = section("Status");
        root.append(&headline_section);

        let groups = gtk4::Box::new(Orientation::Vertical, 18);
        root.append(&groups);

        let (entity_section, entities) = section("Devices");
        root.append(&entity_section);

        TechnicalView {
            root,
            facts,
            errors,
            headline,
            groups,
            entities,
            built: RefCell::new(String::new()),
            rows: RefCell::new(HashMap::new()),
        }
    }

    pub fn widget(&self) -> ScrolledWindow {
        scroller(&self.root)
    }

    pub fn render(&self, snapshot: &TargetSnapshot) {
        let mut facts = Vec::new();
        if !snapshot.distro.is_empty() {
            facts.push(snapshot.distro.clone());
        }
        if !snapshot.kernel.is_empty() {
            facts.push(format!("kernel {}", snapshot.kernel));
        }
        if !snapshot.arch.is_empty() {
            facts.push(snapshot.arch.clone());
        }
        if snapshot.cpu_count > 0 {
            facts.push(format!("{} cores", snapshot.cpu_count));
        }
        // The batching guarantee, made observable in the running app rather than only in a test.
        facts.push(format!("{} round trips", snapshot.round_trips));
        self.facts.set_text(&facts.join(" · "));

        if snapshot.source_errors.is_empty() {
            self.errors.set_visible(false);
        } else {
            // A collector that failed to parse is not fatal and is not hidden either. Discarding
            // it is how a reading silently stops existing.
            self.errors.set_visible(true);
            self.errors.set_text(&format!(
                "Not read this refresh: {}",
                snapshot.source_errors.join("; ")
            ));
        }

        let signature = self.signature(snapshot);
        if *self.built.borrow() != signature {
            self.rebuild(snapshot);
            *self.built.borrow_mut() = signature;
        }

        let rows = self.rows.borrow();
        for gauge in snapshot
            .gauges
            .iter()
            .chain(snapshot.detail_groups.iter().flat_map(|g| g.gauges.iter()))
            .chain(snapshot.entities.iter().flat_map(|e| e.gauges.iter()))
        {
            if let Some(row) = rows.get(&gauge.series_id) {
                row.set(gauge);
            }
        }
    }

    fn signature(&self, snapshot: &TargetSnapshot) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for gauge in &snapshot.gauges {
            parts.push(&gauge.series_id);
        }
        for group in &snapshot.detail_groups {
            parts.push(&group.title);
            for gauge in &group.gauges {
                parts.push(&gauge.series_id);
            }
        }
        for entity in &snapshot.entities {
            parts.push(&entity.id);
            for gauge in &entity.gauges {
                parts.push(&gauge.series_id);
            }
        }
        parts.join("|")
    }

    fn rebuild(&self, snapshot: &TargetSnapshot) {
        clear(&self.headline);
        clear(&self.groups);
        clear(&self.entities);
        self.rows.borrow_mut().clear();

        let grid = flow();
        for gauge in &snapshot.gauges {
            let row = GaugeRow::new(gauge);
            grid.insert(row.widget(), -1);
            self.rows.borrow_mut().insert(gauge.series_id.clone(), row);
        }
        self.headline.append(&grid);

        for group in &snapshot.detail_groups {
            let (outer, body) = section(&group.title);
            let grid = flow();
            for gauge in &group.gauges {
                let row = GaugeRow::new(gauge);
                grid.insert(row.widget(), -1);
                self.rows.borrow_mut().insert(gauge.series_id.clone(), row);
            }
            body.append(&grid);
            self.groups.append(&outer);
        }

        // Cores, interfaces, disks and filesystems. Collapsed by default: a Proxmox host has forty
        // of them and expanding the lot on connect buries the readings above.
        for entity in &snapshot.entities {
            if entity.gauges.is_empty() {
                continue;
            }
            let expander =
                gtk4::Expander::new(Some(&format!("{} · {}", entity.display, entity.kind)));
            let body = gtk4::Box::new(Orientation::Vertical, 8);
            body.set_margin_top(8);
            body.set_margin_start(12);

            let grid = flow();
            for gauge in &entity.gauges {
                let row = GaugeRow::new(gauge);
                grid.insert(row.widget(), -1);
                self.rows.borrow_mut().insert(gauge.series_id.clone(), row);
            }
            body.append(&grid);
            expander.set_child(Some(&body));
            self.entities.append(&expander);
        }
    }
}

impl Default for TechnicalView {
    fn default() -> Self {
        TechnicalView::new()
    }
}

/// A wrapping grid of cards.
///
/// Reflow is driven by the width actually available, not by a device class — the case that matters
/// is a window being resized while the app runs.
fn flow() -> gtk4::FlowBox {
    let grid = gtk4::FlowBox::new();
    grid.set_selection_mode(gtk4::SelectionMode::None);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(6);
    grid.set_homogeneous(true);
    grid
}

/// What explains a busy host.
pub struct ProcessTable {
    root: gtk4::Box,
    rows: gtk4::Box,
    built: RefCell<usize>,
    cells: RefCell<Vec<ProcessRow>>,
}

struct ProcessRow {
    command: Label,
    pid: Label,
    cpu: Label,
    memory: Label,
    bar: Bar,
}

impl ProcessTable {
    pub fn new() -> ProcessTable {
        let root = gtk4::Box::new(Orientation::Vertical, 12);
        root.set_margin_top(20);
        root.set_margin_bottom(20);
        root.set_margin_start(20);
        root.set_margin_end(20);

        let heading = Label::new(Some("Busiest processes"));
        heading.add_css_class("heading");
        heading.set_xalign(0.0);
        root.append(&heading);

        let rows = gtk4::Box::new(Orientation::Vertical, 6);
        root.append(&rows);

        ProcessTable {
            root,
            rows,
            built: RefCell::new(0),
            cells: RefCell::new(Vec::new()),
        }
    }

    pub fn widget(&self) -> ScrolledWindow {
        scroller(&self.root)
    }

    pub fn render(&self, snapshot: &TargetSnapshot) {
        if *self.built.borrow() != snapshot.top_processes.len() {
            clear(&self.rows);
            self.cells.borrow_mut().clear();

            for _ in &snapshot.top_processes {
                let row = gtk4::Box::new(Orientation::Vertical, 4);
                row.add_css_class("sg-card");

                let top = gtk4::Box::new(Orientation::Horizontal, 10);
                let command = Label::new(None);
                command.set_xalign(0.0);
                command.set_hexpand(true);
                command.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
                let pid = Label::new(None);
                pid.add_css_class("sg-number");
                pid.add_css_class("sg-dense");
                let cpu = Label::new(None);
                cpu.add_css_class("sg-number");
                let memory = Label::new(None);
                memory.add_css_class("sg-number");
                memory.add_css_class("sg-dense");

                top.append(&command);
                top.append(&pid);
                top.append(&memory);
                top.append(&cpu);
                row.append(&top);

                let bar = Bar::new();
                row.append(bar.widget());
                self.rows.append(&row);

                self.cells.borrow_mut().push(ProcessRow {
                    command,
                    pid,
                    cpu,
                    memory,
                    bar,
                });
            }
            *self.built.borrow_mut() = snapshot.top_processes.len();
        }

        for (process, cells) in snapshot
            .top_processes
            .iter()
            .zip(self.cells.borrow().iter())
        {
            cells.command.set_text(&process.command);
            cells
                .pid
                .set_text(&format!("pid {} · {}", process.pid, process.state));
            cells
                .cpu
                .set_text(&format_value(process.cpu_percent, "%", false));
            set_level_class(&cells.cpu, &process.severity);
            cells
                .memory
                .set_text(&format_value(process.memory_bytes, "B", true));
            // The share of the *whole machine*, which the core worked out: 100% of one core on a
            // twenty-core host is 5% of the box, and drawing it full would be alarming nonsense.
            cells.bar.set(process.machine_fraction, &process.severity);
        }
    }
}

impl Default for ProcessTable {
    fn default() -> Self {
        ProcessTable::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_ffi::{ConnectionState, MetricGauge, SimpleTile};

    fn snapshot_with(tiles: Vec<SimpleTile>, gauges: Vec<MetricGauge>) -> TargetSnapshot {
        let mut snapshot = TargetSnapshot::placeholder("t1", "10.0.0.4", ConnectionState::Online);
        snapshot.simple_tiles = tiles;
        snapshot.gauges = gauges;
        snapshot
    }

    fn tile(metric: &str, fraction: Option<f64>) -> SimpleTile {
        SimpleTile {
            metric: metric.into(),
            name: "Processor".into(),
            value_text: "12%".into(),
            summary: "Barely working".into(),
            fraction,
            level: "ok".into(),
            history: vec![1.0, 2.0],
        }
    }

    #[test]
    fn the_tile_signature_changes_when_a_reading_gains_a_proportion() {
        // A host that gains a swap partition, or a filesystem that starts reporting a total, must
        // rebuild the row rather than keep drawing a number where a ring now belongs.
        let a = snapshot_with(vec![tile("mem_usage", None)], vec![]);
        let b = snapshot_with(vec![tile("mem_usage", Some(0.4))], vec![]);

        let signature = |s: &TargetSnapshot| -> Vec<String> {
            s.simple_tiles
                .iter()
                .map(|t| format!("{}:{}", t.metric, t.fraction.is_some()))
                .collect()
        };
        assert_ne!(signature(&a), signature(&b));
    }
}
