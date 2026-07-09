use crate::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoadedTool {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) description: String,
    pub(crate) schema: Option<String>,
    pub(crate) dynamic_resident: bool,
    pub(crate) pinned: bool,
    pub(crate) last_used: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ToolContext {
    pub(crate) tools: BTreeMap<String, LoadedTool>,
    pub(crate) max_loaded_tools: usize,
    pub(crate) clock: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DynamicToolCache {
    pub(crate) capacity: usize,
    pub(crate) window_capacity: usize,
    pub(crate) clock: u64,
    pub(crate) frequencies: BTreeMap<PathBuf, u64>,
    pub(crate) pinned: BTreeSet<PathBuf>,
    pub(crate) entries: BTreeMap<PathBuf, CachedToolPath>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheSegment {
    Window,
    Main,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CachedToolPath {
    pub(crate) path: PathBuf,
    pub(crate) last_used: u64,
    pub(crate) segment: CacheSegment,
}

impl DynamicToolCache {
    pub(crate) fn with_window_percent(capacity: usize, window_percent: usize) -> Self {
        let capacity = capacity.max(1);
        let window_percent = window_percent.clamp(1, 100);
        let window_capacity = capacity
            .saturating_mul(window_percent)
            .div_ceil(100)
            .max(1)
            .min(capacity);
        Self {
            capacity,
            window_capacity,
            clock: 0,
            frequencies: BTreeMap::new(),
            pinned: BTreeSet::new(),
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    pub(crate) fn is_pinned_path(&self, path: &Path) -> bool {
        self.pinned.contains(path)
    }

    pub(crate) fn load_path(&mut self, path: &Path) {
        self.record_frequency(path);
        if !self.entries.contains_key(path) {
            let path = path.to_path_buf();
            let _old = self.entries.insert(
                path.clone(),
                CachedToolPath {
                    path: path.clone(),
                    last_used: 0,
                    segment: CacheSegment::Window,
                },
            );
            self.admit_window_candidate(&path);
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(entry) = self.entries.get_mut(path) {
            entry.last_used = self.clock;
        }
    }

    pub(crate) fn pin_path(&mut self, path: &Path) {
        let path = path.to_path_buf();
        let _inserted = self.pinned.insert(path.clone());
        self.load_path(&path);
    }

    pub(crate) fn unpin_path(&mut self, path: &Path) -> bool {
        self.pinned.remove(path)
    }

    fn record_frequency(&mut self, path: &Path) {
        let count = self.frequencies.entry(path.to_path_buf()).or_insert(0);
        *count = count.saturating_add(1);
    }

    fn admit_window_candidate(&mut self, current_path: &Path) {
        while self.window_len() > self.window_capacity {
            let Some(candidate) = self.oldest_window_path() else {
                return;
            };
            if self.main_len() < self.main_capacity() {
                if let Some(entry) = self.entries.get_mut(&candidate) {
                    entry.segment = CacheSegment::Main;
                }
                continue;
            }
            let Some(victim) = self.main_victim_path() else {
                return;
            };
            if candidate == current_path {
                if let Some(entry) = self.entries.get_mut(&candidate) {
                    entry.segment = CacheSegment::Main;
                }
            } else if tiny_lfu_admits(
                self.frequency(&candidate),
                self.frequency(&victim),
                self.last_used(&candidate),
                self.last_used(&victim),
            ) {
                let _dropped = self.entries.remove(&victim);
                if let Some(entry) = self.entries.get_mut(&candidate) {
                    entry.segment = CacheSegment::Main;
                }
            } else {
                let _dropped = self.entries.remove(&candidate);
            }
        }

        while self.unpinned_len() > self.capacity {
            let victim = self
                .main_victim_path()
                .filter(|path| path != current_path)
                .or_else(|| {
                    self.oldest_window_path()
                        .filter(|path| path != current_path)
                });
            let Some(victim) = victim else {
                return;
            };
            let _dropped = self.entries.remove(&victim);
        }
    }

    pub(crate) fn unpinned_len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| !self.is_pinned_path(&entry.path))
            .count()
    }

    fn window_len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| {
                entry.segment == CacheSegment::Window && !self.is_pinned_path(&entry.path)
            })
            .count()
    }

    fn main_len(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| {
                entry.segment == CacheSegment::Main && !self.is_pinned_path(&entry.path)
            })
            .count()
    }

    fn main_capacity(&self) -> usize {
        self.capacity.saturating_sub(self.window_capacity).max(1)
    }

    fn oldest_window_path(&self) -> Option<PathBuf> {
        self.entries
            .values()
            .filter(|entry| {
                entry.segment == CacheSegment::Window && !self.is_pinned_path(&entry.path)
            })
            .min_by_key(|entry| (entry.last_used, entry.path.clone()))
            .map(|entry| entry.path.clone())
    }

    fn main_victim_path(&self) -> Option<PathBuf> {
        wtinylfu_victim_path(
            self.entries
                .values()
                .filter(|entry| {
                    entry.segment == CacheSegment::Main && !self.is_pinned_path(&entry.path)
                })
                .map(|entry| {
                    (
                        entry.path.as_path(),
                        self.frequency(&entry.path),
                        entry.last_used,
                    )
                }),
        )
    }

    fn frequency(&self, path: &Path) -> u64 {
        self.frequencies.get(path).copied().unwrap_or(0)
    }

    fn last_used(&self, path: &Path) -> u64 {
        self.entries.get(path).map_or(0, |entry| entry.last_used)
    }
}

pub(crate) fn tiny_lfu_admits(
    candidate_frequency: u64,
    victim_frequency: u64,
    candidate_last_used: u64,
    victim_last_used: u64,
) -> bool {
    candidate_frequency > victim_frequency
        || (candidate_frequency == victim_frequency && candidate_last_used > victim_last_used)
}

pub(crate) fn wtinylfu_victim_path<'a>(
    entries: impl IntoIterator<Item = (&'a Path, u64, u64)>,
) -> Option<PathBuf> {
    entries
        .into_iter()
        .min_by_key(|&(path, hits, last_used)| (hits, last_used, path.to_path_buf()))
        .map(|(path, _hits, _last_used)| path.to_path_buf())
}

impl ToolContext {
    pub(crate) fn new(max_loaded_tools: usize) -> Self {
        Self {
            tools: BTreeMap::new(),
            max_loaded_tools: max_loaded_tools.max(1),
            clock: 0,
        }
    }

    pub(crate) fn insert(&mut self, mut tool: LoadedTool) -> Vec<LoadedTool> {
        self.clock = self.clock.saturating_add(1);
        tool.last_used = self.clock;
        if let Some(existing) = self.tools.get(&tool.name) {
            tool.pinned |= existing.pinned;
            if existing.path == tool.path {
                tool.dynamic_resident |= existing.dynamic_resident;
            }
        }
        let _old = self.tools.insert(tool.name.clone(), tool);
        self.evict_over_limit()
    }

    pub(crate) fn get_mut(&mut self, name: &str) -> Option<&mut LoadedTool> {
        self.tools.get_mut(name)
    }

    pub(crate) fn touch(&mut self, name: &str) {
        if let Some(tool) = self.tools.get_mut(name) {
            self.clock = self.clock.saturating_add(1);
            tool.last_used = self.clock;
        }
    }

    pub(crate) fn remove_unpinned(&mut self, name: &str) -> Result<Option<LoadedTool>, TshError> {
        if self.tools.get(name).is_some_and(|tool| tool.pinned) {
            return Err(TshError::unavailable(format!(
                "{name} is pinned; run `unpin {name}` before unload"
            )));
        }
        Ok(self.tools.remove(name))
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &LoadedTool> {
        self.tools.values()
    }

    pub(crate) fn pinned_values(&self) -> impl Iterator<Item = &LoadedTool> {
        self.tools.values().filter(|tool| tool.pinned)
    }

    fn evict_over_limit(&mut self) -> Vec<LoadedTool> {
        let mut evicted = Vec::new();
        while self.tools.values().filter(|tool| !tool.pinned).count() > self.max_loaded_tools {
            let Some(name) = self
                .tools
                .values()
                .filter(|tool| !tool.pinned)
                .min_by_key(|tool| (tool.last_used, tool.name.clone()))
                .map(|tool| tool.name.clone())
            else {
                break;
            };
            if let Some(tool) = self.tools.remove(&name) {
                evicted.push(tool);
            }
        }
        evicted
    }

    pub(crate) fn from_state(state: cortexfs::TshContextState, max_loaded_tools: usize) -> Self {
        let mut context = Self::new(max_loaded_tools);
        let mut tools = state.tools;
        tools.sort_by_key(|tool| tool.last_used);
        for tool in tools {
            let _evicted = context.insert(LoadedTool {
                name: tool.name,
                path: tool.path,
                description: tool.description,
                schema: tool.schema,
                dynamic_resident: tool.dynamic_resident,
                pinned: tool.pinned,
                last_used: 0,
            });
        }
        context
    }

    pub(crate) fn to_state(&self) -> cortexfs::TshContextState {
        let mut state = cortexfs::TshContextState::default();
        state.tools = self
            .tools
            .values()
            .map(|tool| cortexfs::TshLoadedToolState {
                name: tool.name.clone(),
                path: tool.path.clone(),
                description: tool.description.clone(),
                schema: tool.schema.clone(),
                dynamic_resident: tool.dynamic_resident,
                pinned: tool.pinned,
                last_used: tool.last_used,
            })
            .collect();
        state
    }
}
