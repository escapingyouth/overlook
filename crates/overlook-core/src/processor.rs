
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    pub interface: String,
    pub boot_time: i64,
    pub rx: u64,
    pub tx: u64,
    pub network_id: Option<i64>,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageDelta {
    pub network_id: i64,
    pub rx: u64,
    pub tx: u64,
    pub estimated: bool,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Usage(UsageDelta),
    Dropped { bytes: u64 },
    /// Nothing to record: no traffic, a first sample, a reboot or a counter reset.
    Idle,
}

/// Longest gap between samples still treated as continuous, in seconds.
pub const GAP_THRESHOLD: f64 = 180.0;

pub fn process(
    previous: Option<&Baseline>,
    interface: &str,
    rx: u64,
    tx: u64,
    boot_time: i64,
    network_id: Option<i64>,
    now: f64,
) -> (Outcome, Baseline) {
    let next = Baseline { interface: interface.to_string(), boot_time, rx, tx, network_id, at: now };
    let Some(prev) = previous else { return (Outcome::Idle, next) };
    if (prev.boot_time - boot_time).abs() > 5 || rx < prev.rx || tx < prev.tx {
        return (Outcome::Idle, next);
    }
    let (drx, dtx) = (rx - prev.rx, tx - prev.tx);
    if drx == 0 && dtx == 0 {
        return (Outcome::Idle, next);
    }
    let continuous = now - prev.at <= GAP_THRESHOLD;
    let outcome = match (prev.network_id, network_id) {
        (None, None) => Outcome::Dropped { bytes: drx + dtx },
        (Some(old), Some(new)) if old == new => {
            Outcome::Usage(UsageDelta { network_id: new, rx: drx, tx: dtx, estimated: !continuous, at: now })
        }
        (old, new) if continuous => Outcome::Usage(UsageDelta {
            network_id: old.or(new).unwrap(),
            rx: drx,
            tx: dtx,
            estimated: true,
            at: now,
        }),
        _ => Outcome::Dropped { bytes: drx + dtx },
    };
    (outcome, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;
    fn base(rx: u64, tx: u64, net: Option<i64>) -> Baseline {
        Baseline { interface: "en0".into(), boot_time: 100, rx, tx, network_id: net, at: T0 }
    }
    fn run(prev: Option<&Baseline>, rx: u64, tx: u64, net: Option<i64>, after: f64, boot: i64) -> Outcome {
        process(prev, "en0", rx, tx, boot, net, T0 + after).0
    }
    fn usage(net: i64, rx: u64, tx: u64, est: bool, after: f64) -> Outcome {
        Outcome::Usage(UsageDelta { network_id: net, rx, tx, estimated: est, at: T0 + after })
    }

    #[test] fn measured_delta_on_same_network() {
        assert_eq!(run(Some(&base(1000, 500, Some(1))), 4000, 700, Some(1), 30.0, 100), usage(1, 3000, 200, false, 30.0));
    }
    #[test] fn first_sample_only_sets_baseline() {
        assert_eq!(run(None, 9_999_999, 1, Some(1), 0.0, 100), Outcome::Idle);
    }
    #[test] fn counter_reset_is_not_counted() {
        assert_eq!(run(Some(&base(5000, 5000, Some(1))), 100, 6000, Some(1), 30.0, 100), Outcome::Idle);
    }
    #[test] fn reboot_resets() {
        assert_eq!(run(Some(&base(10, 10, Some(1))), 50, 50, Some(1), 30.0, 9000), Outcome::Idle);
    }
    #[test] fn boot_time_drift_tolerated() {
        assert!(matches!(run(Some(&base(10, 10, Some(1))), 50, 50, Some(1), 30.0, 102), Outcome::Usage(_)));
    }
    #[test] fn idle() {
        assert_eq!(run(Some(&base(10, 10, Some(1))), 10, 10, Some(1), 30.0, 100), Outcome::Idle);
    }
    #[test] fn switch_within_interval_goes_to_previous_network_as_estimate() {
        assert_eq!(run(Some(&base(0, 0, Some(1))), 100, 0, Some(2), 30.0, 100), usage(1, 100, 0, true, 30.0));
    }
    #[test] fn joining_from_no_network_goes_to_new_network_as_estimate() {
        assert_eq!(run(Some(&base(0, 0, None)), 100, 0, Some(2), 30.0, 100), usage(2, 100, 0, true, 30.0));
    }
    #[test] fn long_gap_same_network_is_estimated() {
        assert_eq!(run(Some(&base(0, 0, Some(1))), 100, 50, Some(1), 3600.0, 100), usage(1, 100, 50, true, 3600.0));
    }
    #[test] fn long_gap_across_networks_is_dropped() {
        assert_eq!(run(Some(&base(0, 0, Some(1))), 100, 50, Some(2), 3600.0, 100),
                   Outcome::Dropped { bytes: 150 });
    }
    #[test] fn no_network_is_dropped() {
        assert_eq!(run(Some(&base(0, 0, None)), 100, 0, None, 30.0, 100), Outcome::Dropped { bytes: 100 });
    }
    #[test] fn counters_beyond_32_bits() {
        let big = 6_000_000_000u64;
        assert_eq!(run(Some(&base(big, big, Some(1))), big + 5_000_000_000, big + 1, Some(1), 30.0, 100), usage(1, 5_000_000_000, 1, false, 30.0));
    }
}
