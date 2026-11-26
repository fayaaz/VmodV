//! Pure request, epoch and bounded-cache types for predictive Scene pre-warming.
//!
//! The GPU owners live in `modv-app`; this module only defines the keys and
//! lifetime rules they share: bounded take-once LRU storage plus the epoch that
//! invalidates warm results which finish after the Scene graph changed.

use std::collections::VecDeque;

/// Legacy Point10 warm-cache default. Kept small because warmed entries own GPU
/// resources; callers may choose another bound with [`PrewarmCache::new`].
pub const DEFAULT_SCENE_PREWARM_CACHE_LIMIT: usize = 1;

/// Renderer families whose first live draw can compile pipelines or allocate
/// large simulation targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrewarmKind {
    Point10,
    Isf,
    Milkdrop,
}

/// One distinct heavy module used by a Scene row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrewarmTarget {
    pub kind: PrewarmKind,
    pub name: String,
    /// The module's resolved `quality` prop (`global` when absent).
    ///
    /// Sent with the request so the compositor warms at the same size the
    /// first live draw will use instead of assuming output resolution.
    pub quality: String,
    /// Whether the module follows the global render scale at `global` quality.
    ///
    /// Point10 scenes are always heavy; for other modules this is the
    /// `3d-scenes` category check from the registry metadata.
    pub heavy: bool,
}

/// A mid-dwell request for one row's already-picked next Scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePrewarmRequest {
    pub scene_id: String,
    pub row: usize,
    pub epoch: u64,
    pub modules: Vec<PrewarmTarget>,
}

/// Renderer identity for a warm entry. Size and format are checked when the
/// entry is adopted, not when it is inserted.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrewarmKey {
    pub kind: PrewarmKind,
    pub name: String,
}

/// The GPU dimensions/format a warmed entry was built for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrewarmSignature {
    pub width: u32,
    pub height: u32,
    pub format: String,
}

/// Cache value plus the invalidation epoch that authorised its warm-up.
#[derive(Clone, Debug)]
pub struct WarmEntry<T> {
    pub epoch: u64,
    pub signature: PrewarmSignature,
    pub value: T,
}

/// Bounded LRU of take-once warm entries.
///
/// `has` refreshes recency. `take_compatible` consumes the entry even when its
/// size or format is wrong, so a mismatched template cannot linger. Advancing
/// or clearing the epoch drops every older entry, including results from a
/// warm-up that finishes after invalidation.
pub struct PrewarmCache<T> {
    limit: usize,
    epoch: u64,
    entries: VecDeque<(PrewarmKey, WarmEntry<T>)>,
}

impl<T> PrewarmCache<T> {
    /// A cache holding at most `limit` entries (minimum one).
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            epoch: 0,
            entries: VecDeque::new(),
        }
    }

    /// The epoch new results must carry to be accepted.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Advance the invalidation epoch and drop entries from older epochs.
    /// Re-setting the current epoch is a no-op, so callers can sync every frame.
    pub fn set_epoch(&mut self, epoch: u64) {
        if epoch == self.epoch {
            return;
        }
        self.epoch = epoch;
        self.entries.retain(|(_, entry)| entry.epoch == epoch);
    }

    /// Reconfigure the bound, evicting least-recently-used entries if needed.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
    }

    /// Insert only when `epoch` still authorises the result.
    pub fn insert_at(
        &mut self,
        key: PrewarmKey,
        epoch: u64,
        signature: PrewarmSignature,
        value: T,
    ) {
        if epoch != self.epoch {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(existing, _)| *existing == key)
        {
            self.entries.remove(index);
        }
        self.entries.push_back((
            key,
            WarmEntry {
                epoch,
                signature,
                value,
            },
        ));
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
    }

    /// Whether a current entry exists; refreshes its LRU position.
    pub fn has(&mut self, key: &PrewarmKey) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|(existing, _)| existing == key)
        else {
            return false;
        };
        if let Some(entry) = self.entries.remove(index) {
            self.entries.push_back(entry);
        }
        true
    }

    /// Consume a current entry once, regardless of signature.
    pub fn take(&mut self, key: &PrewarmKey) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|(existing, _)| existing == key)?;
        self.entries.remove(index).map(|(_, entry)| entry.value)
    }

    /// Consume once only when kind/name/size/format all match.
    ///
    /// A key match with a mismatched signature is still removed, so a template
    /// built for another size or format cannot linger and block a later warm.
    pub fn take_compatible(&mut self, key: &PrewarmKey, signature: &PrewarmSignature) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|(existing, _)| existing == key)?;
        let (_, entry) = self.entries.remove(index)?;
        (entry.signature == *signature).then_some(entry.value)
    }

    /// Number of current entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no current entries are retained.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every entry and bump the epoch.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Drop every entry without changing the epoch.
    ///
    /// Used when the GPU targets stop matching (a resize) but results for the
    /// current epoch may still be warmed again afterwards.
    pub fn discard_entries(&mut self) {
        self.entries.clear();
    }
}
