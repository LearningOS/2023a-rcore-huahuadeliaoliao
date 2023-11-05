//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use super::sha::Sha256Builder;
use crate::config::{STRIDE_LIMIT, BIG_STRIDE};
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use lazy_static::*;

use priority_queue::PriorityQueue;
use core::cmp::Ordering;

#[derive(Clone)]
pub struct Stride(u64);

impl Stride {
    pub fn new() -> Self {
        Stride {
            0: 0,
        }
    }
}

impl Stride {
    pub fn update(&mut self, priority:usize) {
        self.0 += BIG_STRIDE / priority as u64;
    }
}

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.0 >= other.0 {
            if (self.0 - other.0) <= STRIDE_LIMIT {
                // strike越大优先级越小
                Some(Ordering::Less)
            }
            else {
                Some(Ordering::Greater)
            }
        }
        else {
            if (other.0 - self.0) <= STRIDE_LIMIT {
                Some(Ordering::Greater)
            }
            else {
                Some(Ordering::Less)
            }
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl Ord for Stride {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl Eq for Stride {}

///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    // ready_queue: VecDeque<Arc<TaskControlBlock>>,
    ready_queue: PriorityQueue<Arc<TaskControlBlock>,Stride,Sha256Builder>
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: PriorityQueue::with_capacity_and_default_hasher(20),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        // self.ready_queue.push_back(task);
        // update task stride
        let mut inner = task.inner_exclusive_access();
        let priority = inner.priority;
        inner.stride.update(priority);
        self.ready_queue.push(task.clone(), inner.stride.clone());
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // self.ready_queue.pop()
        if let Some((tcb,_)) = self.ready_queue.pop() {
            Some(tcb)
        }
        else {
            None
        }
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