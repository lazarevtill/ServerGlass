//! The live sample store.
//!
//! ServerGlass is deliberately live-only: it never opens a time-series database and never writes a
//! sample to disk. What it does keep is a bounded in-memory window per series, because a gauge
//! without its recent history is a number, not a chart — and because the sparklines in the status
//! grid have to come from somewhere.
//!
//! The window is a hard cap, not a target. A host with 64 cores, 40 containers and a dozen disks
//! produces on the order of a thousand series, and an unbounded store would grow without limit for
//! as long as the app is left open.

use std::collections::{HashMap, VecDeque};

use sg_model::{Entity, EntityId, Sample, SeriesDescriptor, SeriesId};

/// Default window: five minutes at a one-second refresh.
pub const DEFAULT_WINDOW: usize = 300;

/// One point on a chart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub at_ms: i64,
    pub value: f64,
}

/// Bounded history plus the current entity tree and series metadata for one target.
#[derive(Debug)]
pub struct LiveStore {
    window: usize,
    history: HashMap<SeriesId, VecDeque<Point>>,
    descriptors: HashMap<SeriesId, SeriesDescriptor>,
    entities: HashMap<EntityId, Entity>,
    /// Text-valued series, which have no numeric history worth keeping.
    text: HashMap<SeriesId, String>,
}

impl Default for LiveStore {
    fn default() -> Self {
        LiveStore::with_window(DEFAULT_WINDOW)
    }
}

impl LiveStore {
    pub fn with_window(window: usize) -> Self {
        LiveStore {
            window: window.max(1),
            history: HashMap::new(),
            descriptors: HashMap::new(),
            entities: HashMap::new(),
            text: HashMap::new(),
        }
    }

    /// Record a tick.
    ///
    /// Entities and descriptors are upserted because sources re-declare them every tick — that is
    /// what lets parsers stay stateless while container and pod sets churn underneath them.
    pub fn ingest(
        &mut self,
        entities: Vec<Entity>,
        descriptors: Vec<SeriesDescriptor>,
        samples: &[Sample],
    ) {
        for entity in entities {
            self.entities.insert(entity.id.clone(), entity);
        }
        for descriptor in descriptors {
            self.descriptors.insert(descriptor.id.clone(), descriptor);
        }
        for sample in samples {
            match sample.value.as_f64() {
                Some(value) => {
                    let series = self.history.entry(sample.series.clone()).or_default();
                    series.push_back(Point {
                        at_ms: sample.at_ms,
                        value,
                    });
                    while series.len() > self.window {
                        series.pop_front();
                    }
                }
                None => {
                    if let sg_model::Value::Text(text) = &sample.value {
                        self.text.insert(sample.series.clone(), text.clone());
                    }
                }
            }
        }
    }

    /// Drop entities absent from `present`, along with their series.
    ///
    /// Without this, a container that exits stays on the dashboard forever with its last reading
    /// frozen in place — which reads as "still running" to anyone glancing at it.
    pub fn retain_entities(&mut self, present: &[EntityId]) {
        let keep: std::collections::HashSet<&EntityId> = present.iter().collect();
        self.entities.retain(|id, _| keep.contains(id));

        let dropped: Vec<SeriesId> = self
            .descriptors
            .iter()
            .filter(|(_, d)| !keep.contains(&d.entity))
            .map(|(id, _)| id.clone())
            .collect();
        for id in dropped {
            self.descriptors.remove(&id);
            self.history.remove(&id);
            self.text.remove(&id);
        }
    }

    pub fn latest(&self, series: &SeriesId) -> Option<Point> {
        self.history.get(series).and_then(|h| h.back()).copied()
    }

    pub fn history(&self, series: &SeriesId) -> &[Point] {
        // `VecDeque` is contiguous here only by luck, so hand back the front slice and let callers
        // that need the whole window use `history_vec`.
        self.history
            .get(series)
            .map(|h| h.as_slices().0)
            .unwrap_or(&[])
    }

    /// The full window in order, oldest first.
    pub fn history_vec(&self, series: &SeriesId) -> Vec<Point> {
        self.history
            .get(series)
            .map(|h| h.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn text(&self, series: &SeriesId) -> Option<&str> {
        self.text.get(series).map(String::as_str)
    }

    pub fn descriptor(&self, series: &SeriesId) -> Option<&SeriesDescriptor> {
        self.descriptors.get(series)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &SeriesDescriptor> {
        self.descriptors.values()
    }

    pub fn entity(&self, id: &EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }

    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// Series belonging to one entity, sorted by metric name for stable UI ordering.
    pub fn series_for(&self, entity: &EntityId) -> Vec<&SeriesDescriptor> {
        let mut found: Vec<_> = self
            .descriptors
            .values()
            .filter(|d| &d.entity == entity)
            .collect();
        found.sort_by(|a, b| a.metric.cmp(&b.metric));
        found
    }

    /// Direct children of an entity, sorted by display name.
    pub fn children_of(&self, parent: &EntityId) -> Vec<&Entity> {
        let mut found: Vec<_> = self
            .entities
            .values()
            .filter(|e| e.parent.as_ref() == Some(parent))
            .collect();
        found.sort_by(|a, b| a.display.cmp(&b.display));
        found
    }

    /// Total points held, for the memory-bound test and for diagnostics.
    pub fn point_count(&self) -> usize {
        self.history.values().map(VecDeque::len).sum()
    }

    pub fn series_count(&self) -> usize {
        self.history.len()
    }

    /// Forget everything. Used when a target disconnects.
    pub fn clear(&mut self) {
        self.history.clear();
        self.descriptors.clear();
        self.entities.clear();
        self.text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::{EntityKind, SourceId, Unit, Value};

    fn descriptor(entity: &EntityId, metric: &str) -> SeriesDescriptor {
        SeriesDescriptor::gauge(
            &SourceId::new("test"),
            entity,
            metric,
            metric,
            Unit::Percent,
        )
    }

    fn host() -> Entity {
        Entity::host("web-01")
    }

    #[test]
    fn keeps_readings_in_arrival_order() {
        let mut store = LiveStore::default();
        let d = descriptor(&host().id, "cpu");

        for (at, value) in [(1_000, 10.0), (2_000, 20.0), (3_000, 30.0)] {
            store.ingest(
                vec![host()],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), at, Value::Float(value))],
            );
        }

        let history = store.history_vec(&d.id);
        assert_eq!(history.len(), 3);
        assert_eq!(history.first().map(|p| p.value), Some(10.0));
        assert_eq!(store.latest(&d.id).map(|p| p.value), Some(30.0));
    }

    /// The window is a hard cap. Left open for a day at one sample a second, an unbounded store
    /// would hold 86,400 points per series across a thousand series.
    #[test]
    fn the_window_bounds_memory_no_matter_how_long_it_runs() {
        let mut store = LiveStore::with_window(10);
        let d = descriptor(&host().id, "cpu");

        for at in 0..1_000 {
            store.ingest(
                vec![],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), at, Value::Float(at as f64))],
            );
        }

        assert_eq!(store.point_count(), 10);
        let history = store.history_vec(&d.id);
        // The window keeps the newest points, not the oldest.
        assert_eq!(history.first().map(|p| p.value), Some(990.0));
        assert_eq!(history.last().map(|p| p.value), Some(999.0));
    }

    #[test]
    fn re_declaring_a_series_every_tick_does_not_duplicate_it() {
        let mut store = LiveStore::default();
        let d = descriptor(&host().id, "cpu");

        for at in 0..5 {
            store.ingest(
                vec![host()],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), at, Value::Float(1.0))],
            );
        }

        assert_eq!(store.descriptors().count(), 1);
        assert_eq!(store.entities().count(), 1);
        assert_eq!(store.series_count(), 1);
    }

    /// A container that exits must leave the dashboard. Otherwise its last reading sits there
    /// frozen, indistinguishable from a running one.
    #[test]
    fn dropping_an_entity_takes_its_series_with_it() {
        let mut store = LiveStore::default();
        let host = host();
        let gone = Entity::child(&host, EntityKind::Container, "old");
        let stays = Entity::child(&host, EntityKind::Container, "new");

        let d_gone = descriptor(&gone.id, "cpu");
        let d_stays = descriptor(&stays.id, "cpu");
        store.ingest(
            vec![host.clone(), gone.clone(), stays.clone()],
            vec![d_gone.clone(), d_stays.clone()],
            &[
                Sample::new(d_gone.id.clone(), 1, Value::Float(1.0)),
                Sample::new(d_stays.id.clone(), 1, Value::Float(2.0)),
            ],
        );
        assert_eq!(store.point_count(), 2);

        store.retain_entities(&[host.id.clone(), stays.id.clone()]);

        assert!(store.entity(&gone.id).is_none());
        assert!(
            store.latest(&d_gone.id).is_none(),
            "series of a removed entity survived"
        );
        assert!(store.descriptor(&d_gone.id).is_none());
        assert_eq!(store.latest(&d_stays.id).map(|p| p.value), Some(2.0));
    }

    #[test]
    fn exposes_the_entity_tree_in_stable_order() {
        let mut store = LiveStore::default();
        let host = host();
        let children: Vec<_> = ["eth1", "eth0", "lo"]
            .iter()
            .map(|n| Entity::child(&host, EntityKind::NetworkInterface, *n))
            .collect();

        let mut all = vec![host.clone()];
        all.extend(children.clone());
        store.ingest(all, vec![], &[]);

        let names: Vec<_> = store
            .children_of(&host.id)
            .iter()
            .map(|e| e.display.clone())
            .collect();
        assert_eq!(
            names,
            vec!["eth0", "eth1", "lo"],
            "children must be ordered deterministically"
        );
    }

    #[test]
    fn series_for_an_entity_are_sorted_by_metric() {
        let mut store = LiveStore::default();
        let host = host();
        let descriptors: Vec<_> = ["tx_bytes", "rx_bytes", "errors"]
            .iter()
            .map(|m| descriptor(&host.id, m))
            .collect();
        store.ingest(vec![host.clone()], descriptors, &[]);

        let metrics: Vec<_> = store
            .series_for(&host.id)
            .iter()
            .map(|d| d.metric.clone())
            .collect();
        assert_eq!(metrics, vec!["errors", "rx_bytes", "tx_bytes"]);
    }

    #[test]
    fn text_values_are_kept_separately_from_numeric_history() {
        let mut store = LiveStore::default();
        let host = host();
        let d = SeriesDescriptor::info(&SourceId::new("test"), &host.id, "kernel", "Kernel");

        store.ingest(
            vec![host],
            vec![d.clone()],
            &[Sample::new(d.id.clone(), 1, Value::Text("6.1.0".into()))],
        );

        assert_eq!(store.text(&d.id), Some("6.1.0"));
        // Text has no numeric history, so it must not occupy a chart slot.
        assert_eq!(store.point_count(), 0);
    }

    #[test]
    fn clearing_releases_everything() {
        let mut store = LiveStore::default();
        let host = host();
        let d = descriptor(&host.id, "cpu");
        store.ingest(
            vec![host],
            vec![d.clone()],
            &[Sample::new(d.id.clone(), 1, Value::Float(1.0))],
        );

        store.clear();
        assert_eq!(store.point_count(), 0);
        assert_eq!(store.entities().count(), 0);
        assert_eq!(store.descriptors().count(), 0);
    }
}
