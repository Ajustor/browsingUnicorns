//! The built-in ad blocker's lists: loaded from the cache at start, checked
//! once a day (at start and when the window gets the focus back), off the
//! UI thread; and its policy, kept in step with the settings.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use domain::Settings;
use infrastructure::blocker::{BlockerHandle, BlockerLists, BlockerPolicy};

pub struct BlockerRunner {
    pub handle: BlockerHandle,
    /// `None` without a data folder: no blocking.
    lists: Option<Arc<BlockerLists>>,
    /// A load or download is running.
    busy: Arc<AtomicBool>,
    waker: Arc<dyn Fn() + Send + Sync>,
    /// The window had the focus at the last frame.
    focused: bool,
    /// The lists may be downloaded and the cache written (not in a private
    /// window, which only reads them).
    refresh: bool,
}

impl BlockerRunner {
    /// Loads the cached engine, then downloads the lists if due and
    /// `refresh` allows it, on a thread.
    pub fn start(
        handle: BlockerHandle,
        lists: Option<BlockerLists>,
        refresh: bool,
        waker: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let runner = Self {
            handle,
            lists: lists.map(Arc::new),
            busy: Arc::default(),
            waker: Arc::new(waker),
            focused: true,
            refresh,
        };
        runner.spawn(true);
        runner
    }

    fn spawn(&self, load_cache: bool) {
        let Some(lists) = self.lists.clone() else {
            return;
        };
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let (handle, busy, waker) = (self.handle.clone(), self.busy.clone(), self.waker.clone());
        let refresh = self.refresh;
        std::thread::spawn(move || {
            let cached = if !load_cache {
                None
            } else if refresh {
                lists.load_cached()
            } else {
                lists.read_cached()
            };
            if let Some(blocker) = cached {
                handle.set(blocker);
                waker();
            }
            if refresh && lists.due() {
                match lists.refresh() {
                    Ok(blocker) => {
                        handle.set(blocker);
                        waker();
                    }
                    Err(e) => log::warn!("ad blocker lists: {e}"),
                }
            }
            busy.store(false, Ordering::Release);
        });
    }

    /// Call each frame: checks the lists when the window gets the focus
    /// back, hands the settings' changes (here or synced) to the hooks.
    pub fn follow(&mut self, ctx: &egui::Context, settings: &Settings) {
        let focused = ctx.input(|i| i.focused);
        if self.refresh && focused && !self.focused && self.lists.as_ref().is_some_and(|l| l.due())
        {
            self.spawn(false);
        }
        self.focused = focused;
        self.follow_settings(settings);
    }

    /// Hands the settings' changes to the hooks.
    pub fn follow_settings(&self, settings: &Settings) {
        if !self
            .handle
            .policy_is(settings.blocker, &settings.blocker_allowlist)
        {
            self.handle
                .set_policy(BlockerPolicy::from_settings(settings));
        }
    }
}
