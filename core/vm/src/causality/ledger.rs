/// Tag an emit id as foreign (0 = "unknown" stays 0).
pub fn foreign_emit_id(id: u64) -> u64 {
    if id == 0 {
        0
    } else {
        id | FOREIGN_EMIT_BIT
    }
}

#[derive(Clone, Debug)]
pub struct CausalityLedger {
    /// Ring buffers, not Vecs: the retention cap makes eviction the steady
    /// state of any long-running process, so removing the oldest record
    /// must be O(1), not a front-of-Vec memmove of the whole window.
    pub writes: std::collections::VecDeque<WriteRecord>,
    pub emits: std::collections::VecDeque<EmitRecord>,
    /// `(frame, write_watermark)` for each `commit()` that replaced the
    /// world with a fork; the watermark is the *absolute* write count at
    /// commit time, which orders the commit against writes *within* a
    /// frame. Writes performed inside forks are invisible to the ledger, so
    /// any value whose newest record predates a commit may actually
    /// originate from the committed timeline — `why()` discloses that seam
    /// honestly instead of presenting pre-fork provenance as the whole truth.
    pub commits: std::collections::VecDeque<(u64, usize)>,
    pub settlements: std::collections::VecDeque<SettlementRecord>,
    pub proposals: std::collections::VecDeque<ProposalRecord>,
    pub resolutions: std::collections::VecDeque<ResolutionRecord>,
    pub relation_assertions: std::collections::VecDeque<RelationAssertionRecord>,
    pub(crate) next_settlement_id: u64,
    pub(crate) next_proposal_id: u64,
    pub(crate) next_resolution_id: u64,
    /// Retention window: the ledger keeps at most this many write and emit
    /// records each, evicting the oldest. Long-running processes must not
    /// OOM by bookkeeping; recent provenance stays in RAM, full history
    /// lives in the trace (`rad run --record` + `rad replay` rebuild it).
    cap: usize,
    /// Number of evicted write records (absolute index of `writes[0]`).
    write_base: usize,
    /// Number of evicted emit records (ids `<= emit_base` are gone).
    emit_base: usize,
    /// Exact count and cryptographic commitment for every record that has
    /// left the bounded in-memory window.
    truncation: ProvenanceTruncation,
}

pub const DEFAULT_RETENTION_CAP: usize = 100_000;

impl Default for CausalityLedger {
    fn default() -> Self {
        CausalityLedger {
            writes: std::collections::VecDeque::new(),
            emits: std::collections::VecDeque::new(),
            commits: std::collections::VecDeque::new(),
            settlements: std::collections::VecDeque::new(),
            proposals: std::collections::VecDeque::new(),
            resolutions: std::collections::VecDeque::new(),
            relation_assertions: std::collections::VecDeque::new(),
            next_settlement_id: 1,
            next_proposal_id: 1,
            next_resolution_id: 1,
            cap: DEFAULT_RETENTION_CAP,
            write_base: 0,
            emit_base: 0,
            truncation: ProvenanceTruncation::default(),
        }
    }
}

const SUMMARY_CAP: usize = 96;
const CHAIN_DEPTH_CAP: usize = 16;

/// Truncate a display string to a bounded summary.
pub fn summarize(s: &str) -> String {
    if s.len() <= SUMMARY_CAP {
        s.to_string()
    } else {
        let mut cut = SUMMARY_CAP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}
impl CausalityLedger {
    #[inline]
    pub(crate) fn push_write_record(&mut self, record: WriteRecord) {
        if self.writes.len() == self.cap {
            let record = self
                .writes
                .pop_front()
                .expect("retention cap matched a non-empty write window");
            self.absorb_evicted_write(&record);
            self.write_base = self.write_base.saturating_add(1);
        }
        self.writes.push_back(record);
    }

    pub(crate) fn write_watermark(&self) -> usize {
        self.write_base.saturating_add(self.writes.len())
    }

    pub(crate) fn writes_since(
        &self,
        watermark: usize,
    ) -> Result<impl Iterator<Item = &WriteRecord>, String> {
        if watermark < self.write_base {
            return Err(format!(
                "provenance retention window advanced past transition watermark {} (oldest retained {})",
                watermark, self.write_base
            ));
        }
        let offset = watermark.saturating_sub(self.write_base);
        Ok(self.writes.iter().skip(offset))
    }

    /// Shrink (or grow) the retention window. Mostly for tests and servers
    /// with tight memory budgets.
    pub fn set_retention_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.evict_overflow();
    }

    fn evict_overflow(&mut self) {
        while self.writes.len() > self.cap {
            let record = self.writes.pop_front().expect("write window is non-empty");
            self.absorb_evicted_write(&record);
            self.write_base = self.write_base.saturating_add(1);
        }
        while self.emits.len() > self.cap {
            let record = self.emits.pop_front().expect("emit window is non-empty");
            self.absorb_evicted_emit(&record);
            self.emit_base = self.emit_base.saturating_add(1);
        }
        while self.commits.len() > self.cap {
            let record = self
                .commits
                .pop_front()
                .expect("commit window is non-empty");
            self.absorb_evicted_commit(record);
        }
        while self.settlements.len() > self.cap {
            let record = self
                .settlements
                .pop_front()
                .expect("settlement window is non-empty");
            self.absorb_evicted_settlement(&record);
        }
        while self.proposals.len() > self.cap {
            let record = self
                .proposals
                .pop_front()
                .expect("proposal window is non-empty");
            self.absorb_evicted_proposal(&record);
        }
        while self.resolutions.len() > self.cap {
            let record = self
                .resolutions
                .pop_front()
                .expect("resolution window is non-empty");
            self.absorb_evicted_resolution(&record);
        }
        while self.relation_assertions.len() > self.cap {
            let record = self
                .relation_assertions
                .pop_front()
                .expect("relation assertion window is non-empty");
            self.absorb_evicted_relation_assertion(&record);
        }
    }

    /// Returns the emit id used by `Cause::Handler` links (1-based; 0 is
    /// reserved for "unknown" and never matches a record). Ids are stable
    /// across retention eviction.
    pub fn record_emit(&mut self, frame: u64, event: &str, payload: String, by: Cause) -> u64 {
        let id = (self.emit_base + self.emits.len()) as u64 + 1;
        self.emits.push_back(EmitRecord {
            id,
            event: event.to_string(),
            frame,
            payload,
            by,
            origin: None,
        });
        self.evict_overflow();
        id
    }

    pub fn record_write(&mut self, record: WriteRecord) {
        self.push_write_record(record);
    }

    /// Record that `commit()` adopted a fork's world in `frame`.
    pub fn record_commit(&mut self, frame: u64) {
        let watermark = self.write_base + self.writes.len();
        self.commits.push_back((frame, watermark));
        self.evict_overflow();
    }

    pub fn truncation(&self) -> &ProvenanceTruncation {
        &self.truncation
    }
}
