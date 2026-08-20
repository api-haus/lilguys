//! Attributed counters, snapshotted beside the logs and pushed over OTLP — see docs/telemetry.md.
//! Fire-and-forget throughout: a dead collector costs the buddy its numbers, never its life.

use crate::config::Telemetry as Settings;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Attribution travelling with one reading. A spend without a guy behind it is a spend nobody can
/// govern, so most of these carry at least `who`.
pub type Attrs<'a> = &'a [(&'static str, &'a str)];

/// Every metric name lives here. A name typed at the call site is a name that drifts.
pub mod name {
    pub const FRAMES: &str = "lilguys.frames";
    pub const FRAME_RENDER: &str = "lilguys.frame.render";
    pub const FRAME_INTERVAL: &str = "lilguys.frame.interval";
    pub const TICK_RATE: &str = "lilguys.tick.rate";
    pub const ON_SCREEN: &str = "lilguys.on_screen";

    pub const OBSERVATIONS: &str = "lilguys.observations";
    pub const GATE: &str = "lilguys.gate";
    pub const REFLEXES: &str = "lilguys.reflexes";
    pub const DRIFT: &str = "lilguys.drift";

    pub const TURNS: &str = "lilguys.turns";
    pub const TURN_DURATION: &str = "lilguys.turn.duration";
    pub const TOKENS: &str = "lilguys.tokens";
    pub const COMPACTIONS: &str = "lilguys.compactions";
    pub const INTENTS: &str = "lilguys.intents";
    pub const REFUSALS: &str = "lilguys.refusals";
    pub const SPEECH: &str = "lilguys.speech";
    pub const DROPS: &str = "lilguys.drops";
}

/// Milliseconds either side of a 60 Hz frame, so a rate that has quietly halved is visible as a
/// second hump rather than a moved average.
const MS_FRAME: &[f64] =
    &[0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.7, 25.0, 33.3, 66.7, 125.0, 250.0, 500.0];
/// A model round trip, from a warm local one to a gateway having a bad day.
const MS_TURN: &[f64] =
    &[10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 30000.0, 60000.0];

/// Every histogram declares its buckets here rather than at the call site, where they drift apart.
const BUCKETS: &[(&str, &[f64])] = &[
    (name::FRAME_RENDER, MS_FRAME),
    (name::FRAME_INTERVAL, MS_FRAME),
    (name::TURN_DURATION, MS_TURN),
];

fn buckets_for(metric: &str) -> &'static [f64] {
    BUCKETS.iter().find(|(n, _)| *n == metric).map(|(_, b)| *b).unwrap_or(MS_FRAME)
}

/// Millisecond histograms are the common case; anything else says so here.
fn unit_for(metric: &str) -> &'static str {
    match metric {
        name::FRAME_RENDER | name::FRAME_INTERVAL | name::TURN_DURATION => "ms",
        name::TICK_RATE => "Hz",
        // A bare "1" on a gauge comes out of the collector as `_ratio`, which this is not.
        name::ON_SCREEN => "{guy}",
        name::TOKENS => "{token}",
        _ => "1",
    }
}

type Key = (&'static str, Vec<(&'static str, String)>);

enum Series {
    Sum(u64),
    Gauge(f64),
    Hist { counts: Vec<u64>, sum: f64, count: u64 },
}

struct Meter {
    started: Instant,
    start_nanos: u64,
    series: Mutex<BTreeMap<Key, Series>>,
    spans: Mutex<Vec<Value>>,
    trace: AtomicU64,
}

static METER: OnceLock<Meter> = OnceLock::new();

fn meter() -> Option<&'static Meter> {
    METER.get()
}

fn key(metric: &'static str, attrs: Attrs) -> Key {
    let mut owned: Vec<(&'static str, String)> =
        attrs.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
    owned.sort_by_key(|(k, _)| *k);
    (metric, owned)
}

/// Adds to a monotonic counter.
pub fn add(metric: &'static str, n: u64, attrs: Attrs) {
    let Some(m) = meter() else { return };
    let Ok(mut series) = m.series.lock() else { return };
    if let Series::Sum(v) = series.entry(key(metric, attrs)).or_insert(Series::Sum(0)) {
        *v += n;
    }
}

/// Adds one to a monotonic counter.
pub fn count(metric: &'static str, attrs: Attrs) {
    add(metric, 1, attrs);
}

/// Overwrites a value that only makes sense as its latest reading.
pub fn gauge(metric: &'static str, value: f64, attrs: Attrs) {
    let Some(m) = meter() else { return };
    let Ok(mut series) = m.series.lock() else { return };
    if let Series::Gauge(v) = series.entry(key(metric, attrs)).or_insert(Series::Gauge(0.0)) {
        *v = value;
    }
}

/// Files one observation into a histogram's buckets.
pub fn record(metric: &'static str, value: f64, attrs: Attrs) {
    let Some(m) = meter() else { return };
    let bounds = buckets_for(metric);
    let Ok(mut series) = m.series.lock() else { return };
    let entry = series.entry(key(metric, attrs)).or_insert_with(|| Series::Hist {
        counts: vec![0; bounds.len() + 1],
        sum: 0.0,
        count: 0,
    });
    if let Series::Hist { counts, sum, count } = entry {
        let bucket = bounds.iter().position(|b| value <= *b).unwrap_or(bounds.len());
        counts[bucket] += 1;
        *sum += value;
        *count += 1;
    }
}

/// One model turn as a span, so a slow provider is visible next to what the turn produced.
pub fn span(metric: &'static str, elapsed_ms: u128, failed: bool, attrs: Attrs) {
    let Some(m) = meter() else { return };
    let end = unix_nanos();
    let start = end.saturating_sub((elapsed_ms as u64).saturating_mul(1_000_000));
    let id = m.trace.fetch_add(1, Ordering::Relaxed);
    let Ok(mut spans) = m.spans.lock() else { return };
    // A cap, because an unreachable collector must not grow a queue until the daemon dies.
    if spans.len() >= 4096 {
        drop(spans);
        count(name::DROPS, &[("where", "span queue")]);
        return;
    }
    spans.push(json!({
        "traceId": hex(scramble(id ^ start), 32),
        "spanId": hex(scramble(id), 16),
        "name": metric,
        "kind": 3,
        "startTimeUnixNano": start.to_string(),
        "endTimeUnixNano": end.to_string(),
        "attributes": otlp_attrs(&attrs.iter().map(|(k, v)| (*k, (*v).to_string())).collect::<Vec<_>>()),
        "status": { "code": if failed { 2 } else { 1 } },
    }));
}

/// Opens the meter and puts its writer on its own thread. Off returns `None` and every call above
/// becomes a no-op, so nothing else needs to know whether telemetry is on.
pub fn init(settings: &Settings, dir: Option<PathBuf>) -> Option<PathBuf> {
    if !settings.enabled {
        return None;
    }
    let started = Instant::now();
    METER
        .set(Meter {
            started,
            start_nanos: unix_nanos(),
            series: Mutex::new(BTreeMap::new()),
            spans: Mutex::new(Vec::new()),
            trace: AtomicU64::new(1),
        })
        .ok()?;

    let path = dir.map(|d| d.join("state.json"));
    let (file, otlp, snapshot, push) =
        (path.clone(), settings.otlp.clone(), settings.snapshot, settings.push);
    let tick = [snapshot, push].into_iter().filter(|d| !d.is_zero()).min()?;

    std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || {
            let (mut wrote, mut pushed) = (Instant::now(), Instant::now());
            let mut complained = false;
            loop {
                std::thread::sleep(tick);
                if !snapshot.is_zero() && wrote.elapsed() >= snapshot {
                    wrote = Instant::now();
                    if let Some(p) = file.as_ref() {
                        let _ = std::fs::write(p, snapshot_json().to_string());
                    }
                }
                let Some(endpoint) = otlp.as_deref() else { continue };
                if push.is_zero() || pushed.elapsed() < push {
                    continue;
                }
                pushed = Instant::now();
                if let Err(e) = flush(endpoint) {
                    // Once. A collector nobody started must not fill the journal all afternoon.
                    if !complained {
                        complained = true;
                        eprintln!("telemetry: {endpoint} unreachable ({e:#}); no further reports");
                    }
                } else {
                    complained = false;
                }
            }
        })
        .ok()?;
    path
}

// ---- the snapshot -----------------------------------------------------------------------------

/// The curated answer to "what is this costing me", pivoted per character, with the raw series
/// underneath so a question nobody anticipated is still answerable.
pub fn snapshot_json() -> Value {
    let Some(m) = meter() else { return json!({ "enabled": false }) };
    let Ok(series) = m.series.lock() else { return json!({ "enabled": false }) };

    let mut guys: BTreeMap<String, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut gate: BTreeMap<String, u64> = BTreeMap::new();
    let mut refusals: BTreeMap<String, u64> = BTreeMap::new();
    let mut drops: BTreeMap<String, u64> = BTreeMap::new();
    let mut raw = serde_json::Map::new();

    let attr = |k: &Key, want: &str| {
        k.1.iter().find(|(a, _)| *a == want).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    for (k, v) in series.iter() {
        let n = match v {
            Series::Sum(n) => *n,
            Series::Hist { count, .. } => *count,
            Series::Gauge(_) => 0,
        };
        let who = attr(k, "who");
        let mut per = |field: &'static str| {
            if !who.is_empty() {
                *guys.entry(who.clone()).or_default().entry(field).or_insert(0) += n;
            }
        };
        match k.0 {
            name::TURNS => per("turns"),
            name::TOKENS => per("tokens"),
            name::SPEECH => per("spoke"),
            name::INTENTS => per("intents"),
            name::REFLEXES => per("reflexes"),
            name::GATE => {
                let verdict = attr(k, "verdict");
                *gate.entry(verdict.clone()).or_insert(0) += n;
                if !who.is_empty() {
                    let field: &'static str = match verdict.as_str() {
                        "ignored" => "ignored",
                        "think" => "thought",
                        _ => "noticed",
                    };
                    *guys.entry(who.clone()).or_default().entry(field).or_insert(0) += n;
                }
            }
            name::REFUSALS => *refusals.entry(attr(k, "reason")).or_insert(0) += n,
            name::DROPS => *drops.entry(attr(k, "where")).or_insert(0) += n,
            _ => {}
        }
        raw.insert(label(k), reading(v));
    }

    json!({
        "uptime_s": m.started.elapsed().as_secs(),
        "guys": guys,
        "gate": gate,
        "refusals": refusals,
        "drops": drops,
        "frames": frames_json(&series),
        "series": raw,
    })
}

/// The frame view, which is the one that shows a rate quietly halving.
fn frames_json(series: &BTreeMap<Key, Series>) -> Value {
    let find = |metric: &str| series.iter().find(|(k, _)| k.0 == metric);
    let hist = |metric: &str| match find(metric) {
        Some((k, Series::Hist { counts, sum, count })) => Some(json!({
            "count": count,
            "mean_ms": if *count > 0 { sum / *count as f64 } else { 0.0 },
            "p50_ms": quantile(counts, buckets_for(k.0), 0.50),
            "p99_ms": quantile(counts, buckets_for(k.0), 0.99),
        })),
        _ => None,
    };
    json!({
        "submitted": match find(name::FRAMES) { Some((_, Series::Sum(n))) => *n, _ => 0 },
        "render": hist(name::FRAME_RENDER),
        "interval": hist(name::FRAME_INTERVAL),
        "tick_hz": match find(name::TICK_RATE) { Some((_, Series::Gauge(v))) => *v, _ => 0.0 },
    })
}

/// Bucket-derived, so it reports the boundary a quantile fell inside rather than a figure it never
/// measured. Reading 16.7 means "at or under one frame", not "exactly 16.7".
fn quantile(counts: &[u64], bounds: &[f64], q: f64) -> f64 {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let want = (total as f64 * q).ceil() as u64;
    let mut seen = 0;
    for (i, c) in counts.iter().enumerate() {
        seen += c;
        if seen >= want {
            return bounds.get(i).copied().unwrap_or_else(|| bounds.last().copied().unwrap_or(0.0));
        }
    }
    bounds.last().copied().unwrap_or(0.0)
}

fn label(k: &Key) -> String {
    match k.1.is_empty() {
        true => k.0.to_string(),
        false => {
            let attrs: Vec<String> = k.1.iter().map(|(a, v)| format!("{a}={v}")).collect();
            format!("{}{{{}}}", k.0, attrs.join(","))
        }
    }
}

fn reading(v: &Series) -> Value {
    match v {
        Series::Sum(n) => json!(n),
        Series::Gauge(g) => json!(g),
        Series::Hist { sum, count, .. } => json!({ "count": count, "sum": sum }),
    }
}

// ---- OTLP over HTTP, encoded as JSON ----------------------------------------------------------

/// OTLP/HTTP accepts `application/json`, which is why this needs no protobuf and no async runtime.
/// Sums and histograms go up cumulative, which is what a Prometheus-shaped store wants.
fn flush(endpoint: &str) -> anyhow::Result<()> {
    let Some(m) = meter() else { return Ok(()) };
    let now = unix_nanos();

    let metrics = {
        let series = m.series.lock().map_err(|_| anyhow::anyhow!("meter poisoned"))?;
        series.iter().map(|(k, v)| otlp_metric(k, v, m.start_nanos, now)).collect::<Vec<_>>()
    };
    post(endpoint, "v1/metrics", json!({ "resourceMetrics": [{
        "resource": resource(),
        "scopeMetrics": [{ "scope": scope(), "metrics": metrics }],
    }]}))?;

    let spans = {
        let mut queue = m.spans.lock().map_err(|_| anyhow::anyhow!("meter poisoned"))?;
        std::mem::take(&mut *queue)
    };
    if !spans.is_empty() {
        // Put them back if the collector refuses them, so a restart of it loses nothing.
        if let Err(e) = post(endpoint, "v1/traces", json!({ "resourceSpans": [{
            "resource": resource(),
            "scopeSpans": [{ "scope": scope(), "spans": spans.clone() }],
        }]})) {
            if let Ok(mut queue) = m.spans.lock() {
                queue.splice(0..0, spans);
            }
            return Err(e);
        }
    }
    Ok(())
}

fn post(endpoint: &str, path: &str, body: Value) -> anyhow::Result<()> {
    let url = format!("{}/{}", endpoint.trim_end_matches('/'), path);
    ureq::post(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .header("content-type", "application/json")
        .send_json(&body)?;
    Ok(())
}

fn resource() -> Value {
    json!({ "attributes": otlp_attrs(&[
        ("service.name", "lilguysd".to_string()),
        ("service.version", env!("CARGO_PKG_VERSION").to_string()),
    ])})
}

fn scope() -> Value {
    json!({ "name": "lilguys", "version": env!("CARGO_PKG_VERSION") })
}

fn otlp_attrs(attrs: &[(&str, String)]) -> Value {
    json!(attrs
        .iter()
        .map(|(k, v)| json!({ "key": k, "value": { "stringValue": v } }))
        .collect::<Vec<_>>())
}

fn otlp_metric(k: &Key, v: &Series, start: u64, now: u64) -> Value {
    let attributes = otlp_attrs(&k.1);
    let (start, now) = (start.to_string(), now.to_string());
    let head = json!({ "name": k.0, "unit": unit_for(k.0) });
    let mut metric = head.as_object().cloned().unwrap_or_default();
    let data = match v {
        Series::Sum(n) => json!({ "sum": {
            "aggregationTemporality": 2,
            "isMonotonic": true,
            "dataPoints": [{ "startTimeUnixNano": start, "timeUnixNano": now,
                             "asInt": n.to_string(), "attributes": attributes }],
        }}),
        Series::Gauge(g) => json!({ "gauge": {
            "dataPoints": [{ "startTimeUnixNano": start, "timeUnixNano": now,
                             "asDouble": g, "attributes": attributes }],
        }}),
        Series::Hist { counts, sum, count } => json!({ "histogram": {
            "aggregationTemporality": 2,
            "dataPoints": [{
                "startTimeUnixNano": start, "timeUnixNano": now,
                "count": count.to_string(), "sum": sum,
                "bucketCounts": counts.iter().map(u64::to_string).collect::<Vec<_>>(),
                "explicitBounds": buckets_for(k.0),
                "attributes": attributes,
            }],
        }}),
    };
    if let Some(obj) = data.as_object() {
        metric.extend(obj.clone());
    }
    Value::Object(metric)
}

// ---- odds and ends ----------------------------------------------------------------------------

fn unix_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// Span and trace ids only have to not collide, and a counter run through this does not. Pulling
/// in a random number generator for sixteen bytes would be the expensive way to be no more correct.
fn scramble(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn hex(seed: u64, digits: usize) -> String {
    let mut out = String::with_capacity(digits);
    let mut x = seed | 1;
    while out.len() < digits {
        out.push_str(&format!("{x:016x}"));
        x = scramble(x);
    }
    out.truncate(digits);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_reports_the_boundary_it_measured() {
        // Ten readings under 1 ms and one long one: p50 sits in a fast bucket, p99 in the slow one.
        let bounds: &[f64] = &[1.0, 10.0, 100.0];
        let counts = vec![10, 0, 1, 0];
        assert_eq!(quantile(&counts, bounds, 0.50), 1.0);
        assert_eq!(quantile(&counts, bounds, 0.99), 100.0);
    }

    #[test]
    fn quantile_of_nothing_is_zero_not_a_panic() {
        assert_eq!(quantile(&[0, 0], &[1.0], 0.5), 0.0);
    }

    #[test]
    fn ids_are_hex_of_the_length_otlp_asks_for() {
        assert_eq!(hex(1, 32).len(), 32);
        assert_eq!(hex(1, 16).len(), 16);
        assert!(hex(7, 32).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn every_histogram_declares_its_own_buckets() {
        for (metric, _) in BUCKETS {
            assert!(!buckets_for(metric).is_empty());
        }
    }
}
