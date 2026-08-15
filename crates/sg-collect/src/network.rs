//! Per-interface network traffic from `/proc/net/dev`.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// Cumulative counters for one interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterfaceStats {
    pub name: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
}

impl InterfaceStats {
    /// The loopback interface. Real on every host and interesting on almost none, so the UI
    /// collapses it by default rather than letting it dominate a traffic chart.
    pub fn is_loopback(&self) -> bool {
        self.name == "lo"
    }

    /// Interfaces that have never carried a byte in either direction — the `veth`/`docker0` noise
    /// on a container host, and freshly created interfaces.
    pub fn is_idle(&self) -> bool {
        self.rx_bytes == 0 && self.tx_bytes == 0
    }
}

/// Parse `/proc/net/dev`.
///
/// The format has two header lines and then `name: <8 rx fields> <8 tx fields>`. The name is
/// right-aligned into a fixed column, so on a host with a long interface name the colon can end up
/// flush against the first number — splitting on the colon rather than on whitespace is what makes
/// this robust.
pub fn parse_net_dev(text: &str) -> Vec<InterfaceStats> {
    let mut out = Vec::new();

    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }

        let n: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();
        if n.len() < 16 {
            continue;
        }

        out.push(InterfaceStats {
            name: name.to_string(),
            rx_bytes: n[0],
            rx_packets: n[1],
            rx_errors: n[2],
            rx_dropped: n[3],
            tx_bytes: n[8],
            tx_packets: n[9],
            tx_errors: n[10],
            tx_dropped: n[11],
        });
    }

    out
}

pub struct NetworkSource {
    descriptor: SourceDescriptor,
}

impl Default for NetworkSource {
    fn default() -> Self {
        NetworkSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.network"),
                display: "Network".into(),
                description: "Per-interface traffic, packets, errors and drops".into(),
                produces: vec![EntityKind::NetworkInterface],
                requires: Requirements::path("/proc/net/dev"),
                default_enabled: true,
            },
        }
    }
}

impl NetworkSource {
    fn request() -> Request {
        Request::read("/proc/net/dev")
    }
}

impl Source for NetworkSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::request()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let Some(text) = responses.text(&Self::request()) else {
            return Ok(());
        };
        let id = &self.descriptor.id;

        let mut host_rx = 0u64;
        let mut host_tx = 0u64;

        for iface in parse_net_dev(text) {
            // Never carried traffic and is not loopback: a veth or bridge stub. Emitting these
            // would bury the real interfaces on any container host.
            if iface.is_idle() && !iface.is_loopback() {
                continue;
            }

            if !iface.is_loopback() {
                host_rx += iface.rx_bytes;
                host_tx += iface.tx_bytes;
            }

            let entity = Entity::child(&ctx.host, EntityKind::NetworkInterface, &iface.name)
                .with_label("loopback", iface.is_loopback().to_string());

            for (metric, display, value, unit) in [
                ("rx_bytes", "Received", iface.rx_bytes, Unit::Bytes),
                ("tx_bytes", "Sent", iface.tx_bytes, Unit::Bytes),
                ("rx_packets", "Packets in", iface.rx_packets, Unit::Packets),
                ("tx_packets", "Packets out", iface.tx_packets, Unit::Packets),
                ("rx_errors", "Errors in", iface.rx_errors, Unit::Count),
                ("tx_errors", "Errors out", iface.tx_errors, Unit::Count),
                ("rx_dropped", "Dropped in", iface.rx_dropped, Unit::Count),
                ("tx_dropped", "Dropped out", iface.tx_dropped, Unit::Count),
            ] {
                out.emit(
                    SeriesDescriptor::counter(id, &entity.id, metric, display, unit),
                    value,
                );
            }

            out.entity(entity);
        }

        // Host-level totals excluding loopback, so the status page has one traffic figure to show
        // without the UI having to know which interface matters.
        let host = &ctx.host.id;
        out.emit(
            SeriesDescriptor::counter(id, host, "net_rx", "Download", Unit::Bytes),
            host_rx,
        );
        out.emit(
            SeriesDescriptor::counter(id, host, "net_tx", "Upload", Unit::Bytes),
            host_tx,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of, HOSTS};

    const SAMPLE: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:     100       2    0    0    0     0          0         0      100       2    0    0    0     0       0          0
  eth0:   41165     225    1    2    0     0          0         0    51833     205    3    4    0     0       0          0
";

    #[test]
    fn parses_interfaces_and_skips_headers() {
        let ifaces = parse_net_dev(SAMPLE);
        assert_eq!(ifaces.len(), 2);

        let eth0 = ifaces.iter().find(|i| i.name == "eth0").unwrap();
        assert_eq!(eth0.rx_bytes, 41165);
        assert_eq!(eth0.rx_packets, 225);
        assert_eq!(eth0.rx_errors, 1);
        assert_eq!(eth0.rx_dropped, 2);
        assert_eq!(eth0.tx_bytes, 51833);
        assert_eq!(eth0.tx_packets, 205);
        assert_eq!(eth0.tx_errors, 3);
        assert_eq!(eth0.tx_dropped, 4);
    }

    /// The name column is fixed-width and right-aligned, so a long name leaves no space before the
    /// numbers. Splitting on whitespace instead of the colon loses these interfaces entirely.
    #[test]
    fn handles_a_long_name_flush_against_its_numbers() {
        let line = "veth1234567890ab:12 1 0 0 0 0 0 0 34 2 0 0 0 0 0 0\n";
        let ifaces = parse_net_dev(line);
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name, "veth1234567890ab");
        assert_eq!(ifaces[0].rx_bytes, 12);
        assert_eq!(ifaces[0].tx_bytes, 34);
    }

    #[test]
    fn ignores_truncated_rows() {
        assert!(parse_net_dev("eth0: 1 2 3\n").is_empty());
        assert!(parse_net_dev("no colon here\n").is_empty());
    }

    #[test]
    fn parses_both_corpora() {
        for host in HOSTS {
            let (ctx, responses) = corpus(host).file("/proc/net/dev").build();
            let out = sink_for(&NetworkSource::default(), &ctx, &responses);

            assert!(
                out.entities.iter().any(|e| e.display == "eth0"),
                "{host}: eth0 not found among {:?}",
                out.entities.iter().map(|e| &e.display).collect::<Vec<_>>()
            );
            assert!(
                value_of(&out, "net_rx").is_some(),
                "{host}: no host-level totals"
            );
        }
    }

    #[test]
    fn host_totals_exclude_loopback() {
        let (ctx, responses) = corpus("debian").literal("/proc/net/dev", SAMPLE).build();
        let out = sink_for(&NetworkSource::default(), &ctx, &responses);

        // Loopback traffic is real but says nothing about the host's link, and on a busy box it
        // dwarfs it.
        assert_eq!(value_of(&out, "net_rx"), Some(41165.0));
        assert_eq!(value_of(&out, "net_tx"), Some(51833.0));
    }

    #[test]
    fn drops_never_used_interfaces_but_keeps_loopback() {
        let text = format!("{SAMPLE}  veth9: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
        let (ctx, responses) = corpus("debian").literal("/proc/net/dev", &text).build();
        let out = sink_for(&NetworkSource::default(), &ctx, &responses);

        let names: Vec<_> = out.entities.iter().map(|e| e.display.as_str()).collect();
        assert!(
            !names.contains(&"veth9"),
            "idle veth should not clutter the interface list"
        );
        assert!(
            names.contains(&"lo"),
            "loopback should be listed even when idle"
        );
        assert!(names.contains(&"eth0"));
    }

    #[test]
    fn traffic_counters_become_byte_rates() {
        let (ctx, responses) = corpus("debian").literal("/proc/net/dev", SAMPLE).build();
        let out = sink_for(&NetworkSource::default(), &ctx, &responses);

        let rx = out
            .descriptors
            .iter()
            .find(|d| d.metric == "net_rx")
            .unwrap();
        assert_eq!(rx.effective_unit(), Unit::BytesPerSecond);
    }
}
