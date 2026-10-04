use super::{MAX_TOKENS, TIME_BUDGET};
use ort::{
    execution_providers::CPUExecutionProvider,
    session::{Session, builder::GraphOptimizationLevel},
};
use std::{path::Path, time::Duration};

/// CPU resource policy, not a model precision label or a measured RAM limit.
/// Both sessions remain resident. Separate recognizers do not share weights;
/// drop the old recognizer before loading a replacement on constrained machines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeuralOptions {
    /// Explicit 1..=4 threads, or min(available_parallelism, 2) when None.
    /// This controls ORT's pool, not threads in an externally supplied OpenMP runtime.
    pub intra_threads: Option<usize>,
    pub cpu_arena: bool,
    pub memory_pattern: bool,
    pub prepacking: bool,
    pub spinning: bool,
    /// 0 disables optimization; 1..=3 are ORT graph optimization levels.
    pub optimization_level: u8,
    pub max_tokens: usize,
    /// Cooperative budget checked between ORT calls, not a hard deadline.
    pub time_budget: Duration,
}

impl Default for NeuralOptions {
    fn default() -> Self {
        Self::low_memory()
    }
}

impl NeuralOptions {
    pub fn low_memory() -> Self {
        Self {
            intra_threads: None,
            cpu_arena: false,
            memory_pattern: false,
            // Local short synthetic 1+1 INT8 measurements: enabling prepacking
            // added ~2.4 MiB peak but reduced mean time from 2.10s to 1.72s.
            // This is not universal; retain the switch for other models/inputs/CPUs.
            prepacking: true,
            spinning: false,
            // Keep existing graph fusions; lowering this is not automatically
            // a memory win and can increase full-prefix decoder work.
            optimization_level: 3,
            max_tokens: MAX_TOKENS,
            time_budget: TIME_BUDGET,
        }
    }

    /// Original implementation: Level3, intra=4, inter=1, sequential,
    /// memory patterns/prepacking/spinning enabled. In ort rc10 the original
    /// CPUExecutionProvider::default() already disabled the CPU arena.
    pub fn baseline() -> Self {
        Self {
            intra_threads: Some(4),
            memory_pattern: true,
            prepacking: true,
            spinning: true,
            ..Self::low_memory()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.intra_threads.is_some_and(|n| !(1..=4).contains(&n)) {
            return Err("intra_threads must be 1..=4 or None (auto)".into());
        }
        if self.optimization_level > 3 {
            return Err("optimization_level must be 0..=3".into());
        }
        if !(1..=MAX_TOKENS).contains(&self.max_tokens) {
            return Err("max_tokens must be 1..=256".into());
        }
        if self.time_budget.is_zero() || self.time_budget > TIME_BUDGET {
            return Err("time_budget must be > 0 and <= 60 seconds".into());
        }
        Ok(())
    }

    pub fn effective_intra_threads(&self) -> Result<usize, String> {
        self.validate()?;
        Ok(self.intra_threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .min(2)
        }))
    }

    pub(super) fn make_session(&self, path: &Path) -> Result<Session, String> {
        let threads = self.effective_intra_threads()?;
        let level = match self.optimization_level {
            0 => GraphOptimizationLevel::Disable,
            1 => GraphOptimizationLevel::Level1,
            2 => GraphOptimizationLevel::Level2,
            _ => GraphOptimizationLevel::Level3,
        };
        let build = || -> ort::Result<Session> {
            Session::builder()?
                .with_execution_providers([CPUExecutionProvider::default()
                    .with_arena_allocator(self.cpu_arena)
                    .build()
                    .error_on_failure()])?
                .with_intra_threads(threads)?
                .with_inter_threads(1)?
                .with_parallel_execution(false)?
                .with_intra_op_spinning(self.spinning)?
                .with_inter_op_spinning(self.spinning)?
                .with_memory_pattern(self.memory_pattern)?
                .with_prepacking(self.prepacking)?
                .with_optimization_level(level)?
                .commit_from_file(path)
        };
        build().map_err(|e| format!("加载 {} 失败：{e}", path.display()))
    }
}
