//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);
    /// Query: is locked
    fn is_locked(&self) -> bool;
    /// Query: owner tid, if any
    fn owner_tid(&self) -> Option<usize>;
    /// Query: waiter tids (may be empty for spin mutex)
    fn waiters(&self) -> Vec<usize>;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    inner: UPSafeCell<MutexSpinInner>,
}

pub struct MutexSpinInner {
    locked: bool,
    owner_tid: Option<usize>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexSpinInner {
                    locked: false,
                    owner_tid: None,
                })
            },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut inner = self.inner.exclusive_access();
            if inner.locked {
                drop(inner);
                suspend_current_and_run_next();
                continue;
            } else {
                inner.locked = true;
                inner.owner_tid = Some(
                    current_task()
                        .unwrap()
                        .inner_exclusive_access()
                        .res
                        .as_ref()
                        .unwrap()
                        .tid,
                );
                return;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut inner = self.inner.exclusive_access();
        inner.locked = false;
        inner.owner_tid = None;
    }

    fn is_locked(&self) -> bool {
        self.inner.exclusive_access().locked
    }
    fn owner_tid(&self) -> Option<usize> {
        self.inner.exclusive_access().owner_tid
    }
    fn waiters(&self) -> Vec<usize> {
        Vec::new()
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
    owner_tid: Option<usize>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                    owner_tid: None,
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            mutex_inner.owner_tid = Some(
                current_task()
                    .unwrap()
                    .inner_exclusive_access()
                    .res
                    .as_ref()
                    .unwrap()
                    .tid,
            );
        }
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            // pass ownership to waking task
            let tid = waking_task
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .tid;
            mutex_inner.owner_tid = Some(tid);
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
            mutex_inner.owner_tid = None;
        }
    }

    fn is_locked(&self) -> bool {
        self.inner.exclusive_access().locked
    }
    fn owner_tid(&self) -> Option<usize> {
        self.inner.exclusive_access().owner_tid
    }
    fn waiters(&self) -> Vec<usize> {
        let inner = self.inner.exclusive_access();
        inner
            .wait_queue
            .iter()
            .map(|tcb| {
                tcb.inner_exclusive_access()
                    .res
                    .as_ref()
                    .unwrap()
                    .tid
            })
            .collect()
    }
}
