//! Replaceable experiment execution backends.
//!
//! In-memory simulation is CPU-bound and can use Rayon. Future Docker/K8s/stub
//! orchestration will likely need an async Tokio-based executor.

use rayon::prelude::*;

/// Executes independent experiment jobs sequentially.
pub fn run_sequential<T, R, F>(items: Vec<T>, mut f: F) -> anyhow::Result<Vec<R>>
where
    F: FnMut(T) -> anyhow::Result<R>,
{
    items.into_iter().map(|item| f(item)).collect()
}

/// Executes independent experiment jobs in parallel using Rayon.
pub fn run_rayon<T, R, F>(items: Vec<T>, f: F) -> anyhow::Result<Vec<R>>
where
    T: Send,
    R: Send,
    F: Fn(T) -> anyhow::Result<R> + Send + Sync,
{
    items.into_par_iter().map(f).collect()
}

/// Backend-neutral executor trait for future richer runners.
pub trait ExperimentExecutor<T, R> {
    /// Stable executor name.
    fn name(&self) -> &'static str;

    /// Runs a batch of jobs.
    fn run_many(&self, jobs: Vec<T>) -> anyhow::Result<Vec<R>>;
}
