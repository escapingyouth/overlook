use std::collections::HashMap;

/// Turns per-socket cumulative counters (Linux) or per-event sizes (Windows,
/// one unique `socket` per event) into per-pid totals that survive socket close.
#[derive(Default)]
pub struct Acc {
    closed: HashMap<u32, (u64, u64)>,
    live: HashMap<u64, (u32, u64, u64)>,
    round: HashMap<u64, (u32, u64, u64)>,
}

impl Acc {
    pub fn observe(&mut self, socket: u64, pid: u32, rx: u64, tx: u64) {
        // A reused socket id (new pid or counters went backwards) is a new socket: bank the old one.
        if let Some(&(p, r, t)) = self.live.get(&socket) {
            if p != pid || rx < r || tx < t {
                self.live.remove(&socket);
                self.close(p, r, t);
            }
        }
        self.round.insert(socket, (pid, rx, tx));
    }

    pub fn end_round(&mut self) {
        let gone: Vec<_> = self.live.iter().filter(|(s, _)| !self.round.contains_key(s)).map(|(_, v)| *v).collect();
        for (p, r, t) in gone { self.close(p, r, t); }
        self.live = std::mem::take(&mut self.round);
    }

    /// Forget pids that exited so a reused pid starts from zero.
    pub fn retain(&mut self, alive: impl Fn(u32) -> bool) {
        self.closed.retain(|p, _| alive(*p));
        self.live.retain(|_, (p, _, _)| alive(*p));
    }

    pub fn totals(&self) -> HashMap<u32, (u64, u64)> {
        let mut out = self.closed.clone();
        for &(p, r, t) in self.live.values() {
            let e = out.entry(p).or_default();
            e.0 += r;
            e.1 += t;
        }
        out
    }

    fn close(&mut self, pid: u32, rx: u64, tx: u64) {
        let e = self.closed.entry(pid).or_default();
        e.0 += rx;
        e.1 += tx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_socket_bytes_are_kept() {
        let mut a = Acc::default();
        a.observe(1, 5, 100, 10);
        a.end_round();
        a.end_round();
        assert_eq!(a.totals(), HashMap::from([(5, (100, 10))]));
    }

    #[test]
    fn totals_monotonic_across_rounds() {
        let mut a = Acc::default();
        a.observe(1, 5, 100, 10);
        a.end_round();
        assert_eq!(a.totals()[&5], (100, 10));
        a.observe(1, 5, 250, 20);
        a.end_round();
        assert_eq!(a.totals()[&5], (250, 20));
        a.end_round();
        assert_eq!(a.totals()[&5], (250, 20));
    }

    #[test]
    fn per_event_sizes_add_up() {
        let mut a = Acc::default();
        a.observe(1, 7, 100, 0);
        a.observe(2, 7, 0, 30);
        a.end_round();
        a.observe(3, 7, 50, 0);
        a.end_round();
        assert_eq!(a.totals()[&7], (150, 30));
    }

    #[test]
    fn reused_socket_id_is_a_new_socket() {
        let mut a = Acc::default();
        a.observe(1, 5, 100, 10);
        a.end_round();
        a.observe(1, 5, 20, 2);
        a.end_round();
        assert_eq!(a.totals()[&5], (120, 12));
    }

    #[test]
    fn retain_drops_dead_pids() {
        let mut a = Acc::default();
        a.observe(1, 5, 100, 10);
        a.observe(2, 6, 1, 1);
        a.end_round();
        a.end_round();
        a.retain(|pid| pid == 6);
        assert_eq!(a.totals(), HashMap::from([(6, (1, 1))]));
    }
}
