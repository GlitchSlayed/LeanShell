//! Launch history for ranking frequently and recently launched apps higher.

use crate::util;
pub use leanshell_core::history::History as CoreHistory;

#[derive(Default)]
pub struct History {
    inner: CoreHistory,
}

impl History {
    pub fn load() -> Self {
        let path = util::data_dir().join("history.toml");
        History {
            inner: CoreHistory::load_from_path(path),
        }
    }

    pub fn record(&mut self, id: &str) {
        self.inner.record(id, util::now_secs());
    }

    pub fn boost(&self, id: &str, now: u64) -> u32 {
        self.inner.boost(id, now)
    }
}
