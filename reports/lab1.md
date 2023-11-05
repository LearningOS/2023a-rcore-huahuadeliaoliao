rCore实验一
1. 修改数据结构

在 rCore 操作系统中，要实现 task_info 系统调用，首先需要修改任务控制块 TaskControlBlock 中的数据结构，以包含所需的信息。以下是修改后的 TaskControlBlock 结构：

pub struct TaskControlBlock {
    /// 任务生命周期中的状态
    pub task_status: TaskStatus,
    /// 任务上下文
    pub task_cx: TaskContext,
    /// 系统调用次数
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// 进程开始运行的时间
    start_time: usize,
}

此外，添加了一个 new 方法，用于创建新的 TaskControlBlock 实例：

impl TaskControlBlock {
    pub fn new(task_status: TaskStatus, task_cx: TaskContext) -> Self {
        TaskControlBlock {
            task_status,
            task_cx,
            syscall_times: [0u32; MAX_SYSCALL_NUM],
            start_time: 0,
        }
    }
}

这个 new 方法用于简化旧的任务控制块创建方式，并在 TASK_MANAGER 中使用。
2. 更新系统调用次数信息

要追踪系统调用次数，需要在 trap_handler 中处理用户环境调用（UserEnvCall）的情况，并增加一个用于更新当前任务系统调用次数的方法 update_current_syscall_count。以下是相应的代码片段：

rust

match scause.cause() {
    Trap::Exception(Exception::UserEnvCall) => {
        // 跳转到下一条指令
        cx.sepc += 4;
        // 更新当前任务的系统调用次数
        update_current_syscall_count(cx.x[17]);
        // 获取系统调用返回值
        cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
    }
    // 其他处理逻辑
    // ...
}

此外，在 task 模块中以及 TaskManager 中增加了相应的方法来更新系统调用次数。
3. 设置进程第一次被调度的时刻

在内核的调度函数 run_next_task 中增加了一个简单的判断逻辑，以确定是否是进程的第一次被调度，并在需要时初始化 start_time。
4. 实现 sys_task_info 系统调用

最后，实现了 sys_task_info 系统调用，该系统调用允许用户获取当前任务的信息。在 TaskManager 中实现了主体逻辑，包括获取任务信息、状态、系统调用次数以及运行时间。
