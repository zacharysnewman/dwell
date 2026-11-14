//! The renderer's pipelines, made lazily (bug #71). On a cold shader cache the browser's GPU
//! process compiles each pipeline when it is created, and while it compiles the page paints no
//! frame: making all of them in `Renderer::with_samples` (38) froze the loading screen for ~9 s
//! (M-series Mac, Chrome on Metal). So a pipeline is a [`Lazy`]: compiled on its first use, or by
//! [`Warmup`] a few a frame while the loading screen is up (a painted frame between batches); one
//! the chosen tiers never draw is never compiled.
//!
//! The vocabulary is fixed: [`Lazy::new`] (how to make it), [`Lazy::get`] (or deref: made now if it
//! was not), [`Lazy::built`], [`Warmup::step`], and [`created`] (how many this thread has made).

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::atomic::Ordering::Relaxed;

thread_local! {
    static CREATED: Cell<usize> = const { Cell::new(0) };
}

/// How many render pipelines this thread has created (through [`create`]).
pub fn created() -> usize {
    CREATED.with(Cell::get)
}

/// Creates a render pipeline now, counting it.
pub fn create(
    device: &wgpu::Device,
    desc: &wgpu::RenderPipelineDescriptor<'_>,
) -> wgpu::RenderPipeline {
    CREATED.with(|c| c.set(c.get() + 1));
    device.create_render_pipeline(desc)
}

type Make<T> = Box<dyn FnOnce() -> T>;

/// A pipeline (or a part holding pipelines) made on its first use.
pub struct Lazy<T = wgpu::RenderPipeline> {
    cell: OnceCell<T>,
    make: RefCell<Option<Make<T>>>,
}

impl<T> Lazy<T> {
    /// What `make` creates (through [`create`]) when it is first needed.
    pub fn new(make: impl FnOnce() -> T + 'static) -> Self {
        Self {
            cell: OnceCell::new(),
            make: RefCell::new(Some(Box::new(make))),
        }
    }

    /// It, created now if it was not yet.
    pub fn get(&self) -> &T {
        self.cell.get_or_init(|| {
            let make = self.make.borrow_mut().take();
            make.expect("a lazy pipeline is made once")()
        })
    }

    /// Whether it has been created.
    pub fn built(&self) -> bool {
        self.cell.get().is_some()
    }
}

impl<T> std::ops::Deref for Lazy<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.get()
    }
}

/// Builds the pipelines a frame draws a batch at a time, the next batch only once the GPU has
/// done the last (a queue fence), so the GPU process's compile comes in pieces the page paints
/// between rather than piling up.
#[derive(Default)]
pub struct Warmup {
    done: Cell<bool>,
    waiting: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Warmup {
    /// Pipelines compiled per batch.
    pub const PER_FRAME: usize = 1;

    /// Builds up to [`Self::PER_FRAME`] of `wanted` not built yet, unless the GPU is still on the
    /// last batch; whether all of them are built (and the GPU is done with them).
    pub fn step<'a>(
        &self,
        queue: &wgpu::Queue,
        wanted: impl IntoIterator<Item = &'a Lazy>,
    ) -> bool {
        if self.done.get() {
            return true;
        }
        if self.waiting.load(Relaxed) {
            return false;
        }
        let mut made = 0;
        for p in wanted {
            if made == Self::PER_FRAME {
                break;
            }
            if !p.built() {
                p.get();
                made += 1;
            }
        }
        if made == 0 {
            self.done.set(true);
            return true;
        }
        self.waiting.store(true, Relaxed);
        let waiting = self.waiting.clone();
        queue.on_submitted_work_done(move || waiting.store(false, Relaxed));
        queue.submit([]);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lazy_part_is_made_once_on_first_use() {
        let made = std::rc::Rc::new(Cell::new(0));
        let m = made.clone();
        let p = Lazy::new(move || {
            m.set(m.get() + 1);
            7
        });
        assert!(!p.built());
        assert_eq!(made.get(), 0);
        assert_eq!(*p, 7);
        assert_eq!(*p.get(), 7);
        assert!(p.built());
        assert_eq!(made.get(), 1);
    }
}
