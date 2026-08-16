//! The drawing primitives, and the rule that picks between them.
//!
//! Every number these widgets render arrives already formatted, already bounded and already judged
//! by `sg-ffi`. What is decided here is geometry: how big the ring is, where the line goes. That is
//! the whole of a view layer's business.
//!
//! Each widget owns its state behind an `Rc` and is *updated* rather than rebuilt. A refresh
//! arrives once a second, and tearing the widget tree down that often would reset the scroll
//! position under whoever is reading it and drop keyboard focus out of the command box.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::cairo::Context;
use gtk4::gdk::RGBA;
use gtk4::prelude::*;
use gtk4::{DrawingArea, Label, Orientation, Overlay};

use sg_ffi::{format_value, sparkline_points, MetricGauge};

use crate::palette;

/// How a reading should be drawn.
///
/// Invariant 4: a ring implies a proportion, so it is only ever drawn for a reading that has a
/// real maximum. The first build drew a ring for every host-level series and a twenty-core box
/// rendered forty identical tiles, with `tcp_orphaned: 0` given the same visual weight as CPU.
///
/// The choice comes from the shape of the gauge the core handed over — whether it has a maximum,
/// and what its unit is — never from the metric's name. A name-based rule silently mis-draws the
/// first metric somebody adds.
#[derive(Debug, PartialEq, Eq)]
pub enum Shape {
    /// A percentage of something. Ring.
    Proportion,
    /// A quantity out of a known total. Horizontal bar.
    Capacity,
    /// A rate with no ceiling. Number and sparkline.
    Rate,
}

impl Shape {
    pub fn of(gauge: &MetricGauge) -> Shape {
        match gauge.max {
            Some(_) if gauge.unit_suffix == "%" => Shape::Proportion,
            Some(_) => Shape::Capacity,
            None => Shape::Rate,
        }
    }
}

struct RingState {
    fraction: f64,
    accent: RGBA,
    track: RGBA,
}

/// A ring gauge with the value in the middle.
pub struct Ring {
    root: Overlay,
    value: Label,
    caption: Label,
    state: Rc<RefCell<RingState>>,
    area: DrawingArea,
}

impl Ring {
    pub fn new(size: i32) -> Ring {
        let state = Rc::new(RefCell::new(RingState {
            fraction: 0.0,
            accent: palette::accent("checking"),
            track: palette::muted("checking"),
        }));

        let area = DrawingArea::new();
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_hexpand(true);

        let drawn = Rc::clone(&state);
        area.set_draw_func(move |_, cr, width, height| {
            let state = drawn.borrow();
            let width = width as f64;
            let height = height as f64;
            // Proportional stroke, so the same widget reads correctly as a headline tile and as a
            // small one beside a list row.
            let thickness = (width.min(height) * 0.11).max(3.0);
            let radius = ((width.min(height) - thickness) / 2.0).max(1.0);
            let (cx, cy) = (width / 2.0, height / 2.0);

            cr.set_line_width(thickness);
            cr.set_line_cap(gtk4::cairo::LineCap::Round);

            set_source(cr, state.track);
            cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
            cr.stroke().expect("cairo ring track");

            if state.fraction > 0.0 {
                // Twelve o'clock, clockwise: cairo measures angles from three o'clock.
                let start = -std::f64::consts::FRAC_PI_2;
                set_source(cr, state.accent);
                cr.arc(
                    cx,
                    cy,
                    radius,
                    start,
                    start + state.fraction * std::f64::consts::TAU,
                );
                cr.stroke().expect("cairo ring fill");
            }
        });

        let text = gtk4::Box::new(Orientation::Vertical, 0);
        text.set_valign(gtk4::Align::Center);
        text.set_halign(gtk4::Align::Center);

        let value = Label::new(None);
        value.add_css_class("sg-tile-value");
        let caption = Label::new(None);
        caption.add_css_class("sg-tile-name");
        text.append(&value);
        text.append(&caption);

        let root = Overlay::new();
        root.set_child(Some(&area));
        root.add_overlay(&text);

        Ring {
            root,
            value,
            caption,
            state,
            area,
        }
    }

    pub fn widget(&self) -> &Overlay {
        &self.root
    }

    pub fn set(&self, fraction: f64, level: &str, value_text: &str, caption: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.fraction = fraction.clamp(0.0, 1.0);
            state.accent = palette::accent(level);
            state.track = palette::muted(level);
        }
        self.value.set_text(value_text);
        set_level_class(&self.value, level);
        self.caption.set_text(caption);
        self.caption.set_visible(!caption.is_empty());
        self.area.queue_draw();
    }
}

struct BarState {
    fraction: f64,
    accent: RGBA,
    track: RGBA,
}

/// A horizontal bar for a quantity out of a known total.
pub struct Bar {
    area: DrawingArea,
    state: Rc<RefCell<BarState>>,
}

impl Bar {
    pub fn new() -> Bar {
        let state = Rc::new(RefCell::new(BarState {
            fraction: 0.0,
            accent: palette::accent("checking"),
            track: palette::muted("checking"),
        }));

        let area = DrawingArea::new();
        area.set_content_height(8);
        area.set_hexpand(true);

        let drawn = Rc::clone(&state);
        area.set_draw_func(move |_, cr, width, height| {
            let state = drawn.borrow();
            let width = width as f64;
            let height = height as f64;
            let radius = height / 2.0;

            set_source(cr, state.track);
            rounded(cr, 0.0, 0.0, width, height, radius);
            cr.fill().expect("cairo bar track");

            if state.fraction > 0.0 {
                // Never narrower than the cap radius, or a 0.5% reading draws as a misshapen dot.
                let filled = (width * state.fraction).max(height);
                set_source(cr, state.accent);
                rounded(cr, 0.0, 0.0, filled, height, radius);
                cr.fill().expect("cairo bar fill");
            }
        });

        Bar { area, state }
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    pub fn set(&self, fraction: f64, level: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.fraction = fraction.clamp(0.0, 1.0);
            state.accent = palette::accent(level);
            state.track = palette::muted(level);
        }
        self.area.queue_draw();
    }
}

impl Default for Bar {
    fn default() -> Self {
        Bar::new()
    }
}

struct SparkState {
    points: Vec<f64>,
    accent: RGBA,
}

/// A sparkline over the recent window.
///
/// The normalisation — including the floor that stops a nearly-flat series drawing as a cliff —
/// comes from `sg_ffi::sparkline_points`. This decides only where the pixels go.
pub struct Spark {
    area: DrawingArea,
    state: Rc<RefCell<SparkState>>,
}

impl Spark {
    pub fn new(height: i32) -> Spark {
        let state = Rc::new(RefCell::new(SparkState {
            points: Vec::new(),
            accent: palette::accent("none"),
        }));

        let area = DrawingArea::new();
        area.set_content_height(height);
        area.set_hexpand(true);

        let drawn = Rc::clone(&state);
        area.set_draw_func(move |_, cr, width, height| {
            let state = drawn.borrow();
            if state.points.len() < 2 {
                return;
            }
            let width = width as f64;
            let height = height as f64;
            // Inset by the line width so the extremes are not clipped in half.
            let inset = 1.5;
            let usable = (height - inset * 2.0).max(1.0);
            let step = width / (state.points.len() - 1) as f64;
            // Cairo's origin is top-left; a high reading must draw high.
            let at =
                |index: usize, value: f64| (index as f64 * step, inset + (1.0 - value) * usable);

            cr.move_to(0.0, height);
            for (index, value) in state.points.iter().enumerate() {
                let (x, y) = at(index, *value);
                cr.line_to(x, y);
            }
            cr.line_to(width, height);
            cr.close_path();
            let fill = state.accent;
            set_source(cr, RGBA::new(fill.red(), fill.green(), fill.blue(), 0.14));
            cr.fill().expect("cairo sparkline fill");

            cr.set_line_width(1.6);
            cr.set_line_join(gtk4::cairo::LineJoin::Round);
            set_source(cr, state.accent);
            for (index, value) in state.points.iter().enumerate() {
                let (x, y) = at(index, *value);
                if index == 0 {
                    cr.move_to(x, y);
                } else {
                    cr.line_to(x, y);
                }
            }
            cr.stroke().expect("cairo sparkline");
        });

        Spark { area, state }
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    pub fn set(&self, history: &[f64], level: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.points = sparkline_points(history);
            state.accent = palette::accent(level);
        }
        self.area.queue_draw();
    }
}

/// One reading, drawn as its shape demands, and updated in place thereafter.
pub struct GaugeRow {
    root: gtk4::Box,
    label: Label,
    value: Label,
    bar: Option<Bar>,
    spark: Option<Spark>,
}

impl GaugeRow {
    pub fn new(gauge: &MetricGauge) -> GaugeRow {
        let root = gtk4::Box::new(Orientation::Vertical, 6);
        root.add_css_class("sg-card");

        let heading = gtk4::Box::new(Orientation::Horizontal, 8);
        let label = Label::new(None);
        label.add_css_class("sg-tile-name");
        label.set_xalign(0.0);
        label.set_hexpand(true);

        let value = Label::new(None);
        value.add_css_class("sg-number");
        value.set_xalign(1.0);

        heading.append(&label);
        heading.append(&value);
        root.append(&heading);

        let (bar, spark) = match Shape::of(gauge) {
            Shape::Proportion | Shape::Capacity => {
                let bar = Bar::new();
                root.append(bar.widget());
                (Some(bar), None)
            }
            Shape::Rate => {
                let spark = Spark::new(28);
                root.append(spark.widget());
                (None, Some(spark))
            }
        };

        let row = GaugeRow {
            root,
            label,
            value,
            bar,
            spark,
        };
        row.set(gauge);
        row
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }

    pub fn set(&self, gauge: &MetricGauge) {
        self.label.set_text(&gauge.label);
        self.value.set_text(&format_value(
            gauge.value,
            &gauge.unit_suffix,
            gauge.binary_scaled,
        ));
        set_level_class(&self.value, &gauge.severity);

        if let Some(bar) = &self.bar {
            let max = gauge.max.unwrap_or(0.0);
            bar.set(
                if max > 0.0 { gauge.value / max } else { 0.0 },
                &gauge.severity,
            );
        }
        if let Some(spark) = &self.spark {
            // An unbounded rate has no threshold it could have crossed, so it carries no tint. The
            // core already says so by sending `none`; the palette maps that to the neutral accent.
            spark.set(&gauge.history, &gauge.severity);
        }
    }
}

/// Swap a widget's level class, leaving its other classes alone.
pub fn set_level_class(widget: &impl IsA<gtk4::Widget>, level: &str) {
    let widget = widget.as_ref();
    for class in [
        "sg-ok",
        "sg-busy",
        "sg-problem",
        "sg-offline",
        "sg-checking",
        "sg-neutral",
    ] {
        widget.remove_css_class(class);
    }
    widget.add_css_class(palette::css_class(level));
}

fn set_source(cr: &Context, colour: RGBA) {
    cr.set_source_rgba(
        colour.red() as f64,
        colour.green() as f64,
        colour.blue() as f64,
        colour.alpha() as f64,
    );
}

fn rounded(cr: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    let (right, bottom) = (x + width, y + height);
    cr.new_sub_path();
    cr.arc(
        right - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gauge(unit: &str, max: Option<f64>) -> MetricGauge {
        MetricGauge {
            series_id: "s".into(),
            metric: "m".into(),
            label: "M".into(),
            value: 1.0,
            max,
            unit_suffix: unit.into(),
            binary_scaled: false,
            history: vec![],
            severity: "ok".into(),
        }
    }

    #[test]
    fn a_ring_is_only_ever_drawn_for_a_reading_with_a_real_maximum() {
        // Invariant 4. A rate has no ceiling, so it cannot be a proportion of anything.
        assert_eq!(Shape::of(&gauge("%", Some(100.0))), Shape::Proportion);
        assert_eq!(Shape::of(&gauge("B/s", None)), Shape::Rate);
        assert_eq!(Shape::of(&gauge("ops/s", None)), Shape::Rate);
    }

    #[test]
    fn a_quantity_out_of_a_total_is_a_bar_rather_than_a_ring() {
        assert_eq!(
            Shape::of(&gauge("B", Some(150_000_000_000.0))),
            Shape::Capacity
        );
    }

    #[test]
    fn the_shape_comes_from_the_gauge_not_from_the_metric_name() {
        // A metric this build has never heard of still draws correctly, because nothing here
        // matches on names.
        let mut unknown = gauge("%", Some(100.0));
        unknown.metric = "something_invented_next_year".into();
        assert_eq!(Shape::of(&unknown), Shape::Proportion);
    }
}
