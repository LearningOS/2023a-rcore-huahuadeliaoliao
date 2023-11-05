//! Process management syscalls
use core::mem;

use crate::{
    config::{MAX_SYSCALL_NUM, PAGE_SIZE},
    task::{
        change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, current_user_token, get_current_task_info, current_memory_mmap, current_memory_unmmap,
    }, mm::copy_to_user, timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    pub status: TaskStatus,
    /// The numbers of syscall called by task
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    pub time: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    // get a Vector of mut ref of bytes which may across physical pages,
    // if this happens, it means the physical address of these bytes may 
    // not be continuous
    /*let buffers = translated_byte_buffer(
        current_user_token(),
        _ts as *const u8 , 
        mem::size_of::<TimeVal>()
    );
    let us = get_time_us();
    let tv = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    // get byte slice of tv
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &tv as *const _ as *const u8, 
            mem::size_of::<TimeVal>()
        )};
    // copy data
    let mut start = 0;
    for buffer in buffers {
        let end = start + buffer.len();
        buffer.copy_from_slice(&bytes[start..end]);
        start = end;
    }*/

    let us = get_time_us();
    let tv = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    match copy_to_user(
        current_user_token(), 
        _ts, 
        &tv, 
        mem::size_of::<TimeVal>()) {
        0 => 0,
        _ => -1,
    }
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    if let Some(task_info) = get_current_task_info() {
        copy_to_user(
            current_user_token(), 
            _ti, 
            &task_info, 
            mem::size_of::<TaskInfo>());
        0
    }
    else {
        -1
    }
}

#[allow(unused)]
fn check_map_args(start:usize, len:usize, port:Option<usize>) -> bool {
    if start % PAGE_SIZE != 0 {
        return false;
    }
    match port {
        Some(p) => {
            if p & !0x7 != 0 || p & 0x7 == 0 {
                println!("[Kernel]: check_map_args failed, por={:b}",p);
                false
            }
            else {
                true
            }
        }
        None => true
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap");
    if check_map_args(_start, _len, Some(_port)) 
        && current_memory_mmap(_start, _len,_port) {
        0
    }
    else {
        -1
    }
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if check_map_args(_start, _len, None) 
        && current_memory_unmmap(_start, _len) {
        0
    }
    else {
        -1
    }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
