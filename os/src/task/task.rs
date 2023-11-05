//! Types related to task management

use super::TaskContext;
use crate::config::MAX_SYSCALL_NUM;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// Add the syscall times
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Add The time process start to run, the unit is microsecond
    pub start_time: usize
}

impl TaskControlBlock {
    /// Create TaskControlBlock by given TaskStatus and TaskContext
    pub fn new(task_status: TaskStatus, task_cx: TaskContext) -> Self {
        TaskControlBlock {
            task_status,
            task_cx,
            syscall_times: [0u32; MAX_SYSCALL_NUM],
            start_time: 0
        }
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}