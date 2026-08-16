//! Severity levels to colours, and nothing else.
//!
//! This is the whole of what this file is allowed to do. The core decides *whether* a reading is
//! `ok`, `busy` or a `problem` — every threshold, including the temperature rule and the
//! machine-share arithmetic behind a process bar, lives in `sg-ffi`. Invariant 3 has been broken
//! twice by exactly the shortcut this module exists to prevent: a colour threshold written in the
//! view layer "because it was two lines", then written again on another platform with different
//! numbers, so the same host read amber on a phone and green on a desk.
//!
//! If you find yourself wanting a number here, it belongs in `sg-ffi`.

use gtk4::gdk::RGBA;

/// The accent for a `severity` or `level` string handed over by the core.
///
/// Unknown levels get the neutral accent rather than a panic or a default-to-alarm: the core may
/// grow a level this build has never heard of, and a front-end that crashes or shouts at an
/// unfamiliar string is worse than one that draws it plainly.
pub fn accent(level: &str) -> RGBA {
    match level {
        "ok" => rgb(0x3d, 0xa5, 0x6d),
        "busy" => rgb(0xd9, 0x8a, 0x2b),
        "problem" => rgb(0xd2, 0x4b, 0x4b),
        "offline" => rgb(0x8b, 0x8b, 0x92),
        "checking" => rgb(0x6b, 0x8a, 0xbd),
        // `none` — a rate, which is not a proportion of anything and so has no threshold it could
        // have crossed. DESIGN.md: severity tinting applies only where a fraction is real.
        _ => rgb(0x6d, 0x82, 0x9e),
    }
}

/// The same accent, dimmed for use as a track or fill behind the real thing.
pub fn muted(level: &str) -> RGBA {
    let c = accent(level);
    RGBA::new(c.red(), c.green(), c.blue(), 0.18)
}

/// A CSS class name for a level, so labels can be tinted from the stylesheet.
pub fn css_class(level: &str) -> &'static str {
    match level {
        "ok" => "sg-ok",
        "busy" => "sg-busy",
        "problem" => "sg-problem",
        "offline" => "sg-offline",
        "checking" => "sg-checking",
        _ => "sg-neutral",
    }
}

fn rgb(r: u8, g: u8, b: u8) -> RGBA {
    RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

/// The application stylesheet.
///
/// Kept next to the palette so a colour is defined once. The numbers here are typography and
/// spacing, which are genuinely this layer's business — unlike thresholds, which are not.
pub const STYLE: &str = "
.sg-ok { color: #3da56d; }
.sg-busy { color: #d98a2b; }
.sg-problem { color: #d24b4b; }
.sg-offline { color: #8b8b92; }
.sg-checking { color: #6b8abd; }
.sg-neutral { color: alpha(currentColor, 0.7); }

.sg-headline { font-size: 1.6rem; font-weight: 700; }
.sg-tile-value { font-size: 1.5rem; font-weight: 700; font-feature-settings: 'tnum'; }
.sg-tile-name { font-size: 0.85rem; opacity: 0.7; letter-spacing: 0.04em; }
.sg-tile-summary { font-size: 0.9rem; opacity: 0.8; }

/* Monospaced, tabular figures: a changing value must not make the layout twitch. */
.sg-number { font-family: monospace; font-feature-settings: 'tnum'; }
.sg-dense { font-size: 0.9rem; }

.sg-card {
  background: alpha(currentColor, 0.05);
  border-radius: 12px;
  padding: 14px;
}

.sg-output {
  font-family: monospace;
  font-size: 0.9rem;
  padding: 10px;
}
";
