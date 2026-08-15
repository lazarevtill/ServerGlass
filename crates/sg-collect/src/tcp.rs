//! Socket and TCP statistics from `/proc/net/sockstat` and `/proc/net/snmp`.

use std::collections::HashMap;

use sg_model::{
    EntityKind, ParseResult, Request, Requirements, Responses, SampleSink, SeriesDescriptor, Source,
    SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// Parse `/proc/net/sockstat`, whose lines look like `TCP: inuse 6 orphan 0 tw 0 alloc 9 mem 1`.
///
/// Returned as `protocol -> field -> value`.
pub fn parse_sockstat(text: &str) -> HashMap<String, HashMap<String, u64>> {
    let mut out: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for line in text.lines() {
        let Some((proto, rest)) = line.split_once(':') else { continue };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let entry = out.entry(proto.trim().to_string()).or_default();
        // Fields come in name/value pairs; a trailing unpaired name is ignored.
        for pair in fields.chunks_exact(2) {
            if let Ok(value) = pair[1].parse::<u64>() {
                entry.insert(pair[0].to_string(), value);
            }
        }
    }

    out
}

/// Parse `/proc/net/snmp`, which alternates a header line of field names with a value line.
///
/// Returned as `protocol -> field -> value`. Values are signed because `Tcp: MaxConn` is `-1` when
/// unlimited, and parsing that as unsigned would drop the whole row.
pub fn parse_snmp(text: &str) -> HashMap<String, HashMap<String, i64>> {
    let mut out: HashMap<String, HashMap<String, i64>> = HashMap::new();
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();

    for line in text.lines() {
        let Some((proto, rest)) = line.split_once(':') else { continue };
        let proto = proto.trim().to_string();
        let fields: Vec<&str> = rest.split_whitespace().collect();

        // The first line for a protocol names the columns, the second carries the values.
        match headers.get(&proto) {
            None => {
                headers.insert(proto, fields.into_iter().map(String::from).collect());
            }
            Some(names) => {
                let entry = out.entry(proto).or_default();
                for (name, value) in names.iter().zip(fields) {
                    if let Ok(value) = value.parse::<i64>() {
                        entry.insert(name.clone(), value);
                    }
                }
            }
        }
    }

    out
}

pub struct TcpSource {
    descriptor: SourceDescriptor,
}

impl Default for TcpSource {
    fn default() -> Self {
        TcpSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.tcp"),
                display: "Sockets & TCP".into(),
                description: "Socket counts, established connections and TCP segment rates".into(),
                produces: vec![EntityKind::Host],
                requires: Requirements::path("/proc/net/sockstat"),
                default_enabled: true,
            },
        }
    }
}

impl TcpSource {
    fn sockstat() -> Request {
        Request::read("/proc/net/sockstat")
    }

    fn snmp() -> Request {
        Request::read("/proc/net/snmp")
    }
}

impl Source for TcpSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::sockstat(), Self::snmp()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let id = &self.descriptor.id;
        let host = &ctx.host.id;

        if let Some(sockstat) = responses.text(&Self::sockstat()).map(parse_sockstat) {
            if let Some(used) = sockstat.get("sockets").and_then(|s| s.get("used")) {
                out.emit(
                    SeriesDescriptor::gauge(id, host, "sockets", "Sockets", Unit::Count),
                    *used,
                );
            }
            for (proto, field, metric, display) in [
                ("TCP", "inuse", "tcp_inuse", "TCP in use"),
                ("TCP", "tw", "tcp_timewait", "TCP time-wait"),
                ("TCP", "orphan", "tcp_orphan", "TCP orphaned"),
                ("UDP", "inuse", "udp_inuse", "UDP in use"),
            ] {
                if let Some(value) = sockstat.get(proto).and_then(|s| s.get(field)) {
                    out.emit(
                        SeriesDescriptor::gauge(id, host, metric, display, Unit::Count),
                        *value,
                    );
                }
            }
        }

        if let Some(snmp) = responses.text(&Self::snmp()).map(parse_snmp) {
            let Some(tcp) = snmp.get("Tcp") else { return Ok(()) };

            if let Some(established) = tcp.get("CurrEstab") {
                out.emit(
                    SeriesDescriptor::gauge(id, host, "tcp_established", "Established", Unit::Count),
                    *established,
                );
            }
            for (field, metric, display) in [
                ("InSegs", "tcp_in_segs", "Segments in"),
                ("OutSegs", "tcp_out_segs", "Segments out"),
                ("RetransSegs", "tcp_retrans", "Retransmits"),
                ("ActiveOpens", "tcp_active_opens", "Active opens"),
                ("PassiveOpens", "tcp_passive_opens", "Passive opens"),
            ] {
                // Counters are unsigned by nature; a negative here would mean a malformed row.
                if let Some(value) = tcp.get(field).and_then(|v| u64::try_from(*v).ok()) {
                    out.emit(
                        SeriesDescriptor::counter(id, host, metric, display, Unit::Count),
                        value,
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of, HOSTS};

    #[test]
    fn parses_sockstat_pairs() {
        let stats = parse_sockstat("sockets: used 178\nTCP: inuse 6 orphan 0 tw 3 alloc 9 mem 1\n");
        assert_eq!(stats["sockets"]["used"], 178);
        assert_eq!(stats["TCP"]["inuse"], 6);
        assert_eq!(stats["TCP"]["tw"], 3);
    }

    #[test]
    fn ignores_an_unpaired_trailing_field() {
        let stats = parse_sockstat("TCP: inuse 6 orphan\n");
        assert_eq!(stats["TCP"]["inuse"], 6);
        assert_eq!(stats["TCP"].len(), 1);
    }

    #[test]
    fn pairs_snmp_headers_with_their_value_row() {
        let snmp = parse_snmp(
            "Tcp: RtoAlgorithm MaxConn CurrEstab InSegs\nTcp: 1 -1 4 500\nUdp: InDatagrams\nUdp: 9\n",
        );
        assert_eq!(snmp["Tcp"]["CurrEstab"], 4);
        assert_eq!(snmp["Tcp"]["InSegs"], 500);
        assert_eq!(snmp["Udp"]["InDatagrams"], 9);
    }

    /// `MaxConn` is `-1` for "unlimited". Parsing the row as unsigned would discard every field
    /// on it, including the ones that matter.
    #[test]
    fn a_negative_field_does_not_discard_the_row() {
        let snmp = parse_snmp("Tcp: MaxConn CurrEstab\nTcp: -1 7\n");
        assert_eq!(snmp["Tcp"]["MaxConn"], -1);
        assert_eq!(snmp["Tcp"]["CurrEstab"], 7);
    }

    #[test]
    fn negative_values_are_not_emitted_as_counters() {
        let (ctx, responses) = corpus("debian")
            .literal("/proc/net/snmp", "Tcp: InSegs RetransSegs\nTcp: -5 3\n")
            .build();
        let out = sink_for(&TcpSource::default(), &ctx, &responses);

        assert_eq!(value_of(&out, "tcp_in_segs"), None, "a negative counter is malformed input");
        assert_eq!(value_of(&out, "tcp_retrans"), Some(3.0));
    }

    #[test]
    fn reads_both_corpora() {
        for host in HOSTS {
            let (ctx, responses) =
                corpus(host).file("/proc/net/sockstat").file("/proc/net/snmp").build();
            let out = sink_for(&TcpSource::default(), &ctx, &responses);

            assert!(value_of(&out, "tcp_inuse").is_some(), "{host}: no TCP socket count");
            assert!(value_of(&out, "tcp_in_segs").is_some(), "{host}: no TCP segment counters");
        }
    }

    #[test]
    fn one_missing_file_does_not_suppress_the_other() {
        let (ctx, responses) =
            corpus("debian").file("/proc/net/sockstat").missing("/proc/net/snmp").build();
        let out = sink_for(&TcpSource::default(), &ctx, &responses);

        assert!(value_of(&out, "tcp_inuse").is_some());
        assert!(value_of(&out, "tcp_established").is_none());
    }
}
