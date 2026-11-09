//! WebView2 registrations belong to the UI thread and require explicit cleanup.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::ManuallyDrop;

pub(super) struct HookRegistry<T> {
    // Windows invokes Rust TLS destructors under the loader lock, after other
    // threads have stopped. WebView2 cleanup can wait for those threads forever.
    // Normal window/application teardown explicitly extracts and drops entries;
    // a missed cleanup must leave them for the OS instead of calling COM in TLS.
    entries: RefCell<ManuallyDrop<HashMap<String, T>>>,
    shutting_down: Cell<bool>,
}

impl<T> HookRegistry<T> {
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(ManuallyDrop::new(HashMap::new())),
            shutting_down: Cell::new(false),
        }
    }

    pub fn accepts(&self, label: &str) -> bool {
        !self.shutting_down.get() && !self.entries.borrow().contains_key(label)
    }

    pub fn insert(&self, label: String, hooks: T) -> Option<T> {
        if self.shutting_down.get() {
            return Some(hooks);
        }
        self.entries.borrow_mut().insert(label, hooks)
    }

    pub fn remove(&self, label: &str) -> Option<T> {
        self.entries.borrow_mut().remove(label)
    }

    pub fn take_for_shutdown(&self) -> HashMap<String, T> {
        self.shutting_down.set(true);
        std::mem::take(&mut **self.entries.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Hook(Arc<AtomicUsize>);
    impl Drop for Hook {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn thread_teardown_never_calls_engine_cleanup() {
        thread_local! {
            static HOOKS: HookRegistry<Hook> = HookRegistry::new();
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let thread_drops = drops.clone();
        std::thread::spawn(move || {
            HOOKS.with(|hooks| {
                hooks.insert("main".into(), Hook(thread_drops));
            });
        })
        .join()
        .unwrap();
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "TLS destruction must not call WebView2 after Windows starts process teardown"
        );
    }

    #[test]
    fn window_close_and_shutdown_cleanup_each_hook_once() {
        let registry = HookRegistry::new();
        let drops = Arc::new(AtomicUsize::new(0));
        registry.insert("main".into(), Hook(drops.clone()));
        registry.insert("settings".into(), Hook(drops.clone()));
        drop(registry.remove("settings"));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(registry.take_for_shutdown());
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(registry.take_for_shutdown());
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(!registry.accepts("main"));
        drop(registry.insert("late-page".into(), Hook(drops.clone())));
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn cleanup_can_reenter_the_registry_without_a_borrow_conflict() {
        use std::rc::{Rc, Weak};
        struct ReentrantHook {
            registry: Weak<HookRegistry<ReentrantHook>>,
            drops: Rc<Cell<usize>>,
        }
        impl Drop for ReentrantHook {
            fn drop(&mut self) {
                let registry = self.registry.upgrade().unwrap();
                let _ = registry.accepts("another-window");
                self.drops.set(self.drops.get() + 1);
            }
        }
        let registry = Rc::new(HookRegistry::new());
        let drops = Rc::new(Cell::new(0));
        for label in ["main", "settings"] {
            registry.insert(
                label.into(),
                ReentrantHook {
                    registry: Rc::downgrade(&registry),
                    drops: drops.clone(),
                },
            );
        }
        drop(registry.remove("settings"));
        drop(registry.take_for_shutdown());
        assert_eq!(drops.get(), 2);
    }
}
