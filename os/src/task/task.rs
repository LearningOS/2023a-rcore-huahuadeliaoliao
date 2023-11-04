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
    /// An array to keep track of the number of times each syscall is called by the task.
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// The total time (in milliseconds) the task has spent in user mode.
    pub user_time: usize,
    /// The total time (in milliseconds) the task has spent in kernel mode.
    pub kernel_time: usize,
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