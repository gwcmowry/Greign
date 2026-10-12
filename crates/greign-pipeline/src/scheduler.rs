//! Generation-safe progressive work scheduling and bounded CPU caches.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use thiserror::Error;

/// Monotonically increasing interactive render generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Generation(pub u64);

/// Stable source identity used by scheduler and cache requests.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(pub [u8; 32]);

/// Progressive render quality.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum QualityTier {
    /// Reduced-resolution result intended to update interaction promptly.
    Immediate,
    /// Full requested quality after interaction settles.
    Refined,
}

/// One generation-tagged unit of render intent.
#[derive(Clone, Debug)]
pub struct RenderRequest {
    /// Generation active when this request was created.
    pub generation: Generation,
    /// Immutable source identity.
    pub source: SourceId,
    /// Requested progressive quality.
    pub quality: QualityTier,
    clock: Arc<AtomicU64>,
}

impl RenderRequest {
    /// Whether a newer interaction has superseded this request.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.clock.load(AtomicOrdering::Acquire) != self.generation.0
    }
}

/// Pair of immediate and refined requests sharing one generation.
#[derive(Clone, Debug)]
pub struct ProgressiveRequest {
    /// Reduced-resolution request.
    pub immediate: RenderRequest,
    /// Refinement request.
    pub refinement: RenderRequest,
}

/// Issues generations and invalidates every older request atomically.
#[derive(Clone, Debug, Default)]
pub struct RenderCoordinator {
    clock: Arc<AtomicU64>,
}

impl RenderCoordinator {
    /// Begins one progressive interaction, cancelling prior requests.
    #[must_use]
    pub fn begin(&self, source: SourceId) -> ProgressiveRequest {
        let generation = Generation(self.clock.fetch_add(1, AtomicOrdering::AcqRel) + 1);
        let request = |quality| RenderRequest {
            generation,
            source,
            quality,
            clock: Arc::clone(&self.clock),
        };
        ProgressiveRequest {
            immediate: request(QualityTier::Immediate),
            refinement: request(QualityTier::Refined),
        }
    }

    /// Current generation, or zero before the first request.
    #[must_use]
    pub fn current_generation(&self) -> Generation {
        Generation(self.clock.load(AtomicOrdering::Acquire))
    }
}

/// Presentation guard which rejects completed work from stale generations.
#[derive(Clone, Debug, Default)]
pub struct PresentationSlot<T> {
    presented: Option<(Generation, T)>,
}

impl<T> PresentationSlot<T> {
    /// Presents a value only if the request remains current.
    pub fn try_present(&mut self, request: &RenderRequest, value: T) -> bool {
        if request.is_cancelled() {
            return false;
        }
        self.presented = Some((request.generation, value));
        true
    }

    /// Most recently accepted value.
    #[must_use]
    pub fn current(&self) -> Option<&T> {
        self.presented.as_ref().map(|(_, value)| value)
    }
}

/// Background or interactive work class.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JobKind {
    /// Metadata fast path.
    Metadata,
    /// Embedded or generated thumbnail.
    Thumbnail,
    /// Reduced-resolution interaction result.
    ImmediatePreview,
    /// Full preview refinement.
    PreviewRefinement,
    /// Histogram/statistics work.
    Histogram,
    /// Full export.
    Export,
}

impl JobKind {
    const fn priority(self) -> u8 {
        match self {
            Self::ImmediatePreview => 6,
            Self::Metadata => 5,
            Self::Thumbnail => 4,
            Self::Histogram => 3,
            Self::PreviewRefinement => 2,
            Self::Export => 1,
        }
    }
}

/// Work item returned to a bounded executor.
#[derive(Clone, Debug)]
pub struct ScheduledJob {
    /// Cancellation-aware render intent.
    pub request: RenderRequest,
    /// Work class.
    pub kind: JobKind,
}

#[derive(Clone, Debug)]
struct QueueEntry {
    job: ScheduledJob,
    sequence: Reverse<u64>,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.job.kind.priority() == other.job.kind.priority() && self.sequence == other.sequence
    }
}

impl Eq for QueueEntry {}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.job.kind.priority(), self.sequence).cmp(&(other.job.kind.priority(), other.sequence))
    }
}

/// Priority queue with explicit pending and in-flight bounds.
#[derive(Debug)]
pub struct WorkQueue {
    pending: BinaryHeap<QueueEntry>,
    max_pending: usize,
    max_in_flight: usize,
    in_flight: usize,
    next_sequence: u64,
}

impl WorkQueue {
    /// Creates a non-empty bounded queue.
    #[must_use]
    pub fn new(max_pending: usize, max_in_flight: usize) -> Option<Self> {
        (max_pending > 0 && max_in_flight > 0).then(|| Self {
            pending: BinaryHeap::new(),
            max_pending,
            max_in_flight,
            in_flight: 0,
            next_sequence: 0,
        })
    }

    /// Enqueues work, returning false when the pending bound is reached.
    pub fn enqueue(&mut self, job: ScheduledJob) -> bool {
        self.remove_cancelled();
        if self.pending.len() >= self.max_pending {
            let Some(lowest) = self
                .pending
                .iter()
                .min_by_key(|entry| (entry.job.kind.priority(), entry.sequence))
            else {
                return false;
            };
            if job.kind.priority() <= lowest.job.kind.priority() {
                return false;
            }
            let evicted_sequence = lowest.sequence;
            self.pending
                .retain(|entry| entry.sequence != evicted_sequence);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.pending.push(QueueEntry {
            job,
            sequence: Reverse(sequence),
        });
        true
    }

    /// Starts the highest-priority current job when capacity permits.
    pub fn next_job(&mut self) -> Option<ScheduledJob> {
        if self.in_flight >= self.max_in_flight {
            return None;
        }
        self.remove_cancelled();
        let entry = self.pending.pop()?;
        self.in_flight += 1;
        Some(entry.job)
    }

    /// Releases one executor slot after work finishes or cooperatively exits.
    pub fn complete_one(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    /// Current pending count after stale work is removed.
    pub fn pending_len(&mut self) -> usize {
        self.remove_cancelled();
        self.pending.len()
    }

    /// Current in-flight count.
    #[must_use]
    pub const fn in_flight(&self) -> usize {
        self.in_flight
    }

    fn remove_cancelled(&mut self) {
        if self
            .pending
            .iter()
            .any(|entry| entry.job.request.is_cancelled())
        {
            self.pending
                .retain(|entry| !entry.job.request.is_cancelled());
        }
    }
}

/// One rectangle in a pyramid level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    /// Left coordinate.
    pub x: u32,
    /// Top coordinate.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Tile interior plus its clipped source dependency rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileRegion {
    /// Pyramid level, zero being full resolution.
    pub level: u32,
    /// Result pixels owned by this tile.
    pub interior: Rect,
    /// Input pixels required after applying the operator halo.
    pub source: Rect,
}

/// Computes complete, non-overlapping tile interiors with clipped halos.
#[must_use]
pub fn tile_regions(
    level: u32,
    width: u32,
    height: u32,
    tile_size: u32,
    halo: u32,
) -> Option<Vec<TileRegion>> {
    if width == 0 || height == 0 || tile_size == 0 {
        return None;
    }
    let step = usize::try_from(tile_size).ok()?;
    let mut regions = Vec::new();
    for y in (0..height).step_by(step) {
        for x in (0..width).step_by(step) {
            let interior = Rect {
                x,
                y,
                width: tile_size.min(width - x),
                height: tile_size.min(height - y),
            };
            let source_x = x.saturating_sub(halo);
            let source_y = y.saturating_sub(halo);
            let right = x
                .saturating_add(interior.width)
                .saturating_add(halo)
                .min(width);
            let bottom = y
                .saturating_add(interior.height)
                .saturating_add(halo)
                .min(height);
            regions.push(TileRegion {
                level,
                interior,
                source: Rect {
                    x: source_x,
                    y: source_y,
                    width: right - source_x,
                    height: bottom - source_y,
                },
            });
        }
    }
    Some(regions)
}

/// Pyramid dimensions through the final 1×1 level.
#[must_use]
pub fn pyramid_dimensions(mut width: u32, mut height: u32) -> Option<Vec<(u32, u32)>> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut levels = vec![(width, height)];
    while width > 1 || height > 1 {
        width = width.div_ceil(2);
        height = height.div_ceil(2);
        levels.push((width, height));
    }
    Some(levels)
}

/// Viewport zoom policy expressed in physical display pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewZoom {
    /// Scale the complete oriented image into the viewport.
    Fit,
    /// Physical display pixels per original source pixel; `1.0` is 1:1.
    Scale(f32),
}

/// Pyramid choice for immediate interaction and settled refinement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewportLevel {
    /// Level used after interaction settles.
    pub refinement: u32,
    /// At most one coarser level permitted during rapid interaction.
    pub immediate: u32,
}

/// Selects resolution from physical viewport size and zoom.
///
/// The refinement level is the coarsest power-of-two level that remains at
/// least as large as the displayed image in both dimensions. At 1:1 or above,
/// level zero is always selected.
#[must_use]
pub fn select_viewport_level(
    source_width: u32,
    source_height: u32,
    viewport_width: u32,
    viewport_height: u32,
    zoom: ViewZoom,
) -> Option<ViewportLevel> {
    if source_width == 0 || source_height == 0 || viewport_width == 0 || viewport_height == 0 {
        return None;
    }
    let scale = match zoom {
        ViewZoom::Fit => (f64::from(viewport_width) / f64::from(source_width))
            .min(f64::from(viewport_height) / f64::from(source_height)),
        ViewZoom::Scale(scale) if scale.is_finite() && scale > 0.0 => f64::from(scale),
        ViewZoom::Scale(_) => return None,
    };
    if scale >= 1.0 {
        return Some(ViewportLevel {
            refinement: 0,
            immediate: 0,
        });
    }
    let displayed_width = (f64::from(source_width) * scale).ceil();
    let displayed_height = (f64::from(source_height) * scale).ceil();
    let levels = pyramid_dimensions(source_width, source_height)?;
    let mut refinement = 0_u32;
    for (level, (width, height)) in levels.iter().copied().enumerate().skip(1) {
        if f64::from(width) < displayed_width || f64::from(height) < displayed_height {
            break;
        }
        refinement = u32::try_from(level).ok()?;
    }
    let last = u32::try_from(levels.len().saturating_sub(1)).ok()?;
    Some(ViewportLevel {
        refinement,
        immediate: refinement.saturating_add(1).min(last),
    })
}

/// Ordered pipeline stages used for downstream cache invalidation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PipelineStage {
    /// Normalized/cropped sensor data.
    Sensor,
    /// Demosaiced linear camera RGB.
    Demosaic,
    /// Working-space color conversion.
    Color,
    /// Exposure and scene tone.
    Tone,
    /// Detail operations.
    Detail,
    /// Display/output encoding.
    Output,
}

/// Deterministic identity for one cached stage tile.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheKey {
    /// Immutable source digest.
    pub source: SourceId,
    /// Decoder contract version.
    pub decoder_version: u32,
    /// Stage owning this value.
    pub stage: PipelineStage,
    /// Operator implementation version.
    pub operator_version: u32,
    /// Canonical parameter digest.
    pub parameter_hash: u64,
    /// Digest of the upstream cache key.
    pub upstream_hash: u64,
    /// Pyramid level.
    pub level: u32,
    /// Horizontal tile index.
    pub tile_x: u32,
    /// Vertical tile index.
    pub tile_y: u32,
    /// Working/output color-context digest.
    pub color_context_hash: u64,
}

/// Bounded-cache insertion failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CacheError {
    /// One value exceeds the entire configured cache budget.
    #[error("cache entry exceeds the configured byte budget")]
    EntryTooLarge,
}

#[derive(Debug)]
struct CacheEntry<V> {
    value: V,
    bytes: u64,
    last_used: u64,
}

/// Byte-budgeted least-recently-used cache with deterministic eviction.
#[derive(Debug)]
pub struct BudgetedCache<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
    budget_bytes: u64,
    used_bytes: u64,
    peak_bytes: u64,
    clock: u64,
}

impl<K, V> BudgetedCache<K, V>
where
    K: Clone + Eq + Hash + Ord,
{
    /// Creates a cache with a hard byte budget.
    #[must_use]
    pub fn new(budget_bytes: u64) -> Self {
        Self {
            entries: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            peak_bytes: 0,
            clock: 0,
        }
    }

    /// Inserts a weighted value and returns keys evicted to remain in budget.
    ///
    /// # Errors
    ///
    /// Returns [`CacheError::EntryTooLarge`] when one entry exceeds the full
    /// configured budget.
    pub fn insert(&mut self, key: K, value: V, bytes: u64) -> Result<Vec<K>, CacheError> {
        if bytes > self.budget_bytes {
            return Err(CacheError::EntryTooLarge);
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.used_bytes -= previous.bytes;
        }
        let mut evicted = Vec::new();
        while self.used_bytes.saturating_add(bytes) > self.budget_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(entry_key, entry)| (entry.last_used, *entry_key))
                .map(|(entry_key, _)| entry_key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.used_bytes -= entry.bytes;
                evicted.push(oldest);
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.used_bytes += bytes;
        self.peak_bytes = self.peak_bytes.max(self.used_bytes);
        self.entries.insert(
            key,
            CacheEntry {
                value,
                bytes,
                last_used: self.clock,
            },
        );
        Ok(evicted)
    }

    /// Looks up and marks a value as recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.last_used = self.clock;
        Some(&entry.value)
    }

    /// Current accounted bytes.
    #[must_use]
    pub const fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Highest accounted residency reached.
    #[must_use]
    pub const fn peak_bytes(&self) -> u64 {
        self.peak_bytes
    }

    /// Number of resident entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no entries are resident.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.entries.retain(|key, entry| {
            if keep(key) {
                true
            } else {
                self.used_bytes -= entry.bytes;
                false
            }
        });
    }
}

impl<V> BudgetedCache<CacheKey, V> {
    /// Invalidates only the changed stage and its downstream dependents.
    pub fn invalidate_from(&mut self, changed: PipelineStage) {
        self.retain(|key| key.stage < changed);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BudgetedCache, CacheKey, Generation, JobKind, PipelineStage, PresentationSlot, QualityTier,
        RenderCoordinator, ScheduledJob, SourceId, ViewZoom, ViewportLevel, WorkQueue,
        pyramid_dimensions, select_viewport_level, tile_regions,
    };

    fn source(byte: u8) -> SourceId {
        SourceId([byte; 32])
    }

    fn key(stage: PipelineStage, tile_x: u32) -> CacheKey {
        CacheKey {
            source: source(1),
            decoder_version: 1,
            stage,
            operator_version: 1,
            parameter_hash: 2,
            upstream_hash: 3,
            level: 0,
            tile_x,
            tile_y: 0,
            color_context_hash: 4,
        }
    }

    #[test]
    fn rapid_edits_never_present_a_stale_generation() {
        let coordinator = RenderCoordinator::default();
        let first = coordinator.begin(source(1));
        let second = coordinator.begin(source(1));
        let mut slot = PresentationSlot::default();
        assert!(!slot.try_present(&first.immediate, "stale"));
        assert!(slot.try_present(&second.immediate, "current"));
        assert_eq!(slot.current(), Some(&"current"));
        assert_eq!(coordinator.current_generation(), Generation(2));
    }

    #[test]
    fn switching_images_discards_old_work_and_prioritizes_immediate_preview() {
        let coordinator = RenderCoordinator::default();
        let old = coordinator.begin(source(1));
        let mut queue = WorkQueue::new(8, 1).expect("queue");
        assert!(queue.enqueue(ScheduledJob {
            request: old.refinement,
            kind: JobKind::PreviewRefinement,
        }));
        let current = coordinator.begin(source(2));
        assert!(queue.enqueue(ScheduledJob {
            request: current.refinement.clone(),
            kind: JobKind::PreviewRefinement,
        }));
        assert!(queue.enqueue(ScheduledJob {
            request: current.immediate.clone(),
            kind: JobKind::ImmediatePreview,
        }));
        let next = queue.next_job().expect("current work");
        assert_eq!(next.request.source, source(2));
        assert_eq!(next.request.quality, QualityTier::Immediate);
        assert_eq!(queue.pending_len(), 1);
    }

    #[test]
    fn concurrency_and_pending_work_are_hard_bounded() {
        let coordinator = RenderCoordinator::default();
        let request = coordinator.begin(source(1));
        let mut queue = WorkQueue::new(2, 1).expect("queue");
        for kind in [JobKind::Metadata, JobKind::Thumbnail] {
            assert!(queue.enqueue(ScheduledJob {
                request: request.immediate.clone(),
                kind,
            }));
        }
        assert!(!queue.enqueue(ScheduledJob {
            request: request.refinement,
            kind: JobKind::Export,
        }));
        assert!(queue.next_job().is_some());
        assert!(queue.next_job().is_none());
        assert_eq!(queue.in_flight(), 1);
        queue.complete_one();
        assert!(queue.next_job().is_some());
    }

    #[test]
    fn urgent_preview_replaces_background_work_when_queue_is_full() {
        let coordinator = RenderCoordinator::default();
        let request = coordinator.begin(source(1));
        let mut queue = WorkQueue::new(2, 1).expect("queue");
        for kind in [JobKind::Export, JobKind::Histogram] {
            assert!(queue.enqueue(ScheduledJob {
                request: request.refinement.clone(),
                kind,
            }));
        }
        assert!(queue.enqueue(ScheduledJob {
            request: request.immediate,
            kind: JobKind::ImmediatePreview,
        }));
        assert_eq!(queue.pending_len(), 2);
        assert_eq!(
            queue.next_job().expect("preview").kind,
            JobKind::ImmediatePreview
        );
    }

    #[test]
    fn tiles_cover_odd_dimensions_and_clip_halos() {
        let tiles = tile_regions(2, 17, 11, 8, 3).expect("tiles");
        assert_eq!(tiles.len(), 6);
        assert_eq!(tiles[0].interior.width, 8);
        assert_eq!(tiles[0].source.x, 0);
        assert_eq!(tiles[0].source.width, 11);
        let last = tiles.last().expect("last tile");
        assert_eq!((last.interior.x, last.interior.y), (16, 8));
        assert_eq!((last.interior.width, last.interior.height), (1, 3));
        assert_eq!((last.source.x, last.source.y), (13, 5));
        assert_eq!((last.source.width, last.source.height), (4, 6));
    }

    #[test]
    fn pyramid_reaches_one_pixel_without_losing_odd_edges() {
        assert_eq!(
            pyramid_dimensions(9, 5),
            Some(vec![(9, 5), (5, 3), (3, 2), (2, 1), (1, 1)])
        );
    }

    #[test]
    fn viewport_lod_tracks_screen_size_and_returns_to_original_at_one_to_one() {
        assert_eq!(
            select_viewport_level(6_960, 4_640, 1_920, 1_080, ViewZoom::Fit),
            Some(ViewportLevel {
                refinement: 2,
                immediate: 3,
            })
        );
        assert_eq!(
            select_viewport_level(6_960, 4_640, 3_840, 2_160, ViewZoom::Fit),
            Some(ViewportLevel {
                refinement: 1,
                immediate: 2,
            })
        );
        assert_eq!(
            select_viewport_level(6_960, 4_640, 1_920, 1_080, ViewZoom::Scale(1.0)),
            Some(ViewportLevel {
                refinement: 0,
                immediate: 0,
            })
        );
    }

    #[test]
    fn lru_eviction_never_exceeds_budget() {
        let mut cache = BudgetedCache::new(10);
        cache.insert(1, "one", 4).expect("insert");
        cache.insert(2, "two", 4).expect("insert");
        assert_eq!(cache.get(&1), Some(&"one"));
        let evicted = cache.insert(3, "three", 4).expect("insert");
        assert_eq!(evicted, vec![2]);
        assert_eq!(cache.get(&1), Some(&"one"));
        assert_eq!(cache.get(&2), None);
        assert!(cache.used_bytes() <= 10);
        assert!(cache.peak_bytes() <= 10);
    }

    #[test]
    fn oversized_cache_value_is_refused_without_eviction() {
        let mut cache = BudgetedCache::new(10);
        cache.insert(1, "one", 4).expect("insert");
        assert_eq!(
            cache.insert(2, "oversized", 11),
            Err(super::CacheError::EntryTooLarge)
        );
        assert_eq!(cache.get(&1), Some(&"one"));
        assert_eq!(cache.used_bytes(), 4);
    }

    #[test]
    fn invalidation_removes_only_changed_and_downstream_stages() {
        let mut cache = BudgetedCache::new(1_000);
        for (index, stage) in [
            PipelineStage::Sensor,
            PipelineStage::Demosaic,
            PipelineStage::Color,
            PipelineStage::Tone,
            PipelineStage::Detail,
            PipelineStage::Output,
        ]
        .into_iter()
        .enumerate()
        {
            cache
                .insert(
                    key(stage, u32::try_from(index).expect("small stage index")),
                    stage,
                    10,
                )
                .expect("insert");
        }
        cache.invalidate_from(PipelineStage::Color);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&key(PipelineStage::Sensor, 0)).is_some());
        assert!(cache.get(&key(PipelineStage::Demosaic, 1)).is_some());
        assert!(cache.get(&key(PipelineStage::Color, 2)).is_none());
        assert_eq!(cache.used_bytes(), 20);
    }

    #[test]
    fn repeated_edit_cancel_switch_workload_stays_bounded() {
        let coordinator = RenderCoordinator::default();
        let mut queue = WorkQueue::new(16, 2).expect("queue");
        let mut cache = BudgetedCache::new(128);
        for edit in 0..1_000_u32 {
            let progressive = coordinator.begin(source((edit % 2) as u8));
            let _ = queue.enqueue(ScheduledJob {
                request: progressive.immediate,
                kind: JobKind::ImmediatePreview,
            });
            let _ = queue.enqueue(ScheduledJob {
                request: progressive.refinement,
                kind: JobKind::PreviewRefinement,
            });
            cache
                .insert(key(PipelineStage::Tone, edit), edit, 16)
                .expect("bounded insert");
            assert!(queue.pending_len() <= 16);
            assert!(cache.used_bytes() <= 128);
        }
        assert!(cache.len() <= 8);
    }
}
