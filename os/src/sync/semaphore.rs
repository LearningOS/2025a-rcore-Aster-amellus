//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
    /// per-thread allocation count for deadlock detection
    pub allocations: alloc::vec::Vec<(usize, usize)>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                    allocations: alloc::vec::Vec::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let current_tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        // releasing one unit from current owner if any
        if let Some((_, cnt)) = inner.allocations.iter_mut().find(|(tid, _)| *tid == current_tid) {
            if *cnt > 0 {
                *cnt -= 1;
            }
        }
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                let tid = task
                    .inner_exclusive_access()
                    .res
                    .as_ref()
                    .unwrap()
                    .tid;
                // pass allocation to waking task
                if let Some((_, cnt)) = inner.allocations.iter_mut().find(|(t, _)| *t == tid) {
                    *cnt += 1;
                } else {
                    inner.allocations.push((tid, 1));
                }
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let current_tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        } else {
            // successfully acquired one unit
            if let Some((_, cnt)) = inner.allocations.iter_mut().find(|(tid, _)| *tid == current_tid) {
                *cnt += 1;
            } else {
                inner.allocations.push((current_tid, 1));
            }
        }
    }
}
