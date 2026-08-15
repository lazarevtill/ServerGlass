# ServerGlass design

Why the interface looks the way it does. The rules here are enforced in code, mostly in
`crates/sg-ffi` — because wording and thresholds that live in each front-end drift apart, and on
this product the wording *is* the feature.

---

## The widget must match the metric

A ring implies a proportion of something. Drawing one for "context switches: 26,219/s" tells the
reader nothing, and worse, implies a fullness that does not exist.

| Metric shape | Widget |
|---|---|
| Percentage (has a maximum) | ring gauge |
| Capacity (used of total) | horizontal bar with `used / total` |
| Rate (bytes/s, ops/s) | large monospaced number + sparkline |
| Count / state | plain label–value row |

This rule was learned the hard way. The first build drew a ring for every host-level series, and a
20-core Proxmox host rendered forty identical tiles — `tcp_orphaned: 0` given the same visual
weight as CPU. That is not a dashboard, it is a data dump.

The fix was in two parts: `HEADLINE` in `view.rs` now **curates** rather than merely orders, and
everything it excludes moves into collector-titled groups instead of disappearing.

## Say it once

The default screen carries three readings, not four. Uptime used to be the fourth, but the health
card's own sentence already reads "Running for 13h 52m" — the tile repeated it, had no ring
because uptime has no proportion, and left the grid unbalanced.

Similarly, the all-hosts grid only appears once there is more than one host. With a single server
it navigates to the same answer the page already shows.

## Ordering is semantic, never alphabetical

Sorting detail metrics by name renders load averages as `load1, load15, load5`, and scatters the
memory breakdown instead of running total → used → available. `DETAIL_ORDER` states the order
explicitly; anything unlisted sorts after it, by name.

The headline tiles are likewise fixed, so they do not move when a host gains a swap partition.

## Plain language, in the core

Someone who does not know what SSH is also does not know what a load average is. `plain.rs` turns
readings into sentences, and two rules govern it:

**Never state a problem without stating its size.** "Storage almost full" is an alarm; "Storage is
almost full — the main drive is 92% full" is something to act on.

**Never dress an unrecognised failure up as one that is understood.** Known failures are rewritten
into next steps:

| Raw | Shown |
|---|---|
| `authentication failed for root@10.0.0.4` | The username or key was not accepted. Check the sign-in details are the ones this server expects. |
| `host key … is not in known_hosts` | This is the first time connecting to this server… |
| `host key … CHANGED` | …that can mean it was rebuilt — or that something is impersonating it. Do not reconnect until you know which. |

Anything unmatched keeps its technical detail rather than being given a confident, wrong
explanation. There is a test for that.

`plain_name` returns `None` for metrics with no lay meaning — swap, load averages, context
switches, socket counts. They are real, still collected, and still shown under **Show every
reading**. They are simply not what the first screen is for.

## Charts must not lie in either direction

Sparklines scale to the observed range, not to zero: a byte rate hovering between 4.0 and
4.2 MiB/s drawn against a zero baseline is a flat line that says nothing.

But range-scaling alone lies the other way. Storage sitting at 5.19% and ticking to 5.20% has a
span of 0.01 — stretched to full height, it draws a cliff, and the chart screams that the disk just
filled up. The span is therefore floored at 5% of the magnitude, so genuinely flat series draw
flat and only real movement is amplified.

The same instinct governs colour: severity tinting is applied **only** where a fraction is real.
An unbounded rate gets a neutral accent, because there is no threshold for it to have crossed.

## Layout follows measured width, not device class

The case that matters is a window whose width changes while the app is running: an unfolding
phone, an iPad entering Split View, a resized Mac window. A `GeometryReader` re-evaluates on all of
them; a size class does not always change, and stored state goes stale.

- Below ~680pt the two-column panels stack.
- The three headline tiles are always one row, with the ring sized to the width available. An
  adaptive grid wrapped them as 2 + 1 and left a hole beside the last one.
- Summary text reserves two lines whether or not it needs both, so "Barely working" and
  "240.9 GiB free of 254.2 GiB" produce cards of equal height.

## Foldables get the hinge, not just the width

A foldable is not "sometimes a wide screen". Three things change independently:

1. **Width** — folded it is a phone, unfolded a small tablet. The ordinary size-class question.
2. **The hinge** — a `FoldingFeature` occupies real pixels. When it separates the screen
   vertically, the panes are split *at the hinge* and the seam is left empty. A plain
   `Row(weight(1f))` lays a scrolling column across the fold, where the text is physically bent.
3. **Posture** — half-opened and flat on a desk is a different device again.

The activity also survives folding (`configChanges` in the manifest). Without it the SSH
connection is torn down and every chart restarts from empty mid-gesture.

## One design, four platforms

The palette, thresholds, wording, number formatting and health verdicts all come from Rust. Android
draws its rings and sparklines with Compose `Canvas` rather than approximating the Apple ones,
because someone moving between their phone and their desk should not have to relearn the
dashboard.

What differs per platform is only what should:

| | macOS | iPhone | iPad | Android |
|---|---|---|---|---|
| Navigation | sidebar + detail | navigation stack | split view | one or two panes by width |
| Add-host file picker | `NSOpenPanel` | path field | path field | path field |
| Fold handling | — | — | — | hinge-aware split |

## Density is earned, not assumed

The technical view is deliberately dense — small type, tight spacing, monospaced numbers so columns
align and a changing value does not make the layout twitch. That density is right for someone
triaging a server.

It is wrong as a default. The plain screen is the default, and the dense one is one tap away with
the choice remembered.
