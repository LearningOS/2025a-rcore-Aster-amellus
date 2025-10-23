//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use core::cmp::Ordering;
use lazy_static::*;

/// Wrapper storing stride metadata for binary heap scheduling.
struct StrideTask {
    stride: usize,
    order: usize,
    task: Arc<TaskControlBlock>,
}

impl PartialEq for StrideTask {
    fn eq(&self, other: &Self) -> bool {
        self.stride == other.stride && self.order == other.order
    }
}

impl Eq for StrideTask {}

impl PartialOrd for StrideTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StrideTask {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .stride
            .cmp(&self.stride)
            .then_with(|| other.order.cmp(&self.order))
    }
}

/// Stride-based scheduler maintaining runnable tasks in a min-heap.
pub struct TaskManager {
    ready_queue: BinaryHeap<StrideTask>,
    next_order: usize,
}

impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
            next_order: 0,
        }
    }

    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        let stride = {
            let inner = task.inner_exclusive_access();
            inner.stride
        };
        let order = self.next_order;
        self.next_order = self.next_order.wrapping_add(1);
        self.ready_queue.push(StrideTask {
            stride,
            order,
            task,
        });
    }

    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop().map(|StrideTask { task, .. }| {
            {
                let mut inner = task.inner_exclusive_access();
                inner.stride = inner.stride.checked_add(inner.pass).unwrap_or(inner.pass);
            }
            task
        })
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
