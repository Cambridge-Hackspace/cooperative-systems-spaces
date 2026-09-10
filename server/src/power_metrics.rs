//! Optional per-tool power history (#49): expose the latest draw/voltage a tool
//! reported as Prometheus gauges on the existing `/metrics` registry, so an
//! external Prometheus + Grafana can keep history without a new time-series
//! database. Default off (`[power.timeseries] enabled`).
//!
//! The gauges register into the `dr-metrix` registry the `/metrics` handler
//! serves (its own `Registry`, not the global default), so they only appear
//! when [`init`] is called with that registry at startup. `record` is a no-op
//! until then, so the ingest path can call it unconditionally.
//!
//! Cardinality is bounded: one series per tool, labelled by `tool_id`. A gauge
//! holds its last value, which is the intended "latest reading" semantics; an
//! idle tool reports draw 0 and the gauge follows.

use prometheus::{GaugeVec, Opts, Registry};
use std::sync::OnceLock;

/// The registered gauges. `None`-equivalent (unset `OnceLock`) means the history
/// submodule is disabled, and every `record` is a no-op.
struct PowerGauges {
    draw_amps: GaugeVec,
    voltage: GaugeVec,
}

static GAUGES: OnceLock<PowerGauges> = OnceLock::new();

/// Register the power gauges into the metrics registry, once, if enabled. Called
/// from `main` after the `PrometheusMetrics` registry is built. Safe to call
/// with `enabled = false` (does nothing) and safe to call more than once (only
/// the first registration takes).
pub fn init(registry: &Registry, enabled: bool) -> prometheus::Result<()> {
    if !enabled || GAUGES.get().is_some() {
        return Ok(());
    }
    let draw_amps = GaugeVec::new(
        Opts::new(
            "css_tool_power_draw_amps",
            "Latest reported tool draw, in amps, per tool (#49).",
        ),
        &["tool_id"],
    )?;
    let voltage = GaugeVec::new(
        Opts::new(
            "css_tool_power_voltage",
            "Latest reported tool voltage, per tool (#49).",
        ),
        &["tool_id"],
    )?;
    registry.register(Box::new(draw_amps.clone()))?;
    registry.register(Box::new(voltage.clone()))?;
    // If another thread won the race, ours is simply dropped (unregistered).
    let _ = GAUGES.set(PowerGauges { draw_amps, voltage });
    Ok(())
}

/// Record a tool's latest reading onto the gauges. No-op when the submodule is
/// disabled. `None` values leave the corresponding gauge unchanged.
pub fn record(tool_id: &str, draw_amps: Option<f64>, voltage: Option<f64>) {
    let Some(g) = GAUGES.get() else {
        return;
    };
    if let Some(d) = draw_amps {
        g.draw_amps.with_label_values(&[tool_id]).set(d);
    }
    if let Some(v) = voltage {
        g.voltage.with_label_values(&[tool_id]).set(v);
    }
}
