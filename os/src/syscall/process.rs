//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::TRAMPOLINE,
    loader::get_app_data_by_name,
    mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission, VirtAddr},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, MIN_PRIORITY,
    },
    timer::get_time_us,
};
use core::{mem::size_of, slice};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let task = current_task().unwrap();
    let pid = task.pid.0;
    trace!("kernel:pid[{}] sys_get_time", pid);
    if _ts.is_null() {
        return -1;
    }
    let usec = get_time_us();
    let timeval = TimeVal {
        sec: usec / 1_000_000,
        usec: usec % 1_000_000,
    };
    let token = current_user_token();
    let mut buffers = translated_byte_buffer(token, _ts as *const u8, size_of::<TimeVal>());
    let src = unsafe {
        slice::from_raw_parts(
            &timeval as *const TimeVal as *const u8,
            size_of::<TimeVal>(),
        )
    };
    let mut offset = 0;
    for chunk in buffers.iter_mut() {
        let len = chunk.len();
        chunk.copy_from_slice(&src[offset..offset + len]);
        offset += len;
    }
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    let task = current_task().unwrap();
    let pid = task.pid.0;
    trace!(
        "kernel:pid[{}] sys_mmap start={:#x} len={} prot={:#x}",
        pid,
        _start,
        _len,
        _port
    );
    const VALID_PROT_MASK: usize = 0x7;
    if _len == 0 {
        return -1;
    }
    if _port == 0 || (_port & !VALID_PROT_MASK) != 0 {
        return -1;
    }
    let start_va = VirtAddr::from(_start);
    if !start_va.aligned() {
        return -1;
    }
    let end = match _start.checked_add(_len) {
        Some(end) => end,
        None => return -1,
    };
    if end > TRAMPOLINE {
        return -1;
    }
    let end_va = VirtAddr::from(end);
    let start_vpn = start_va.floor();
    let end_vpn = end_va.ceil();
    if start_vpn >= end_vpn {
        return -1;
    }
    let mut perm = MapPermission::U;
    if (_port & 0x1) != 0 {
        perm |= MapPermission::R;
    }
    if (_port & 0x2) != 0 {
        perm |= MapPermission::W;
    }
    if (_port & 0x4) != 0 {
        perm |= MapPermission::X;
    }
    let mut inner = task.inner_exclusive_access();
    if inner.memory_set.overlaps_with(start_vpn, end_vpn) {
        return -1;
    }
    inner.memory_set.insert_framed_area(start_va, end_va, perm);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    let task = current_task().unwrap();
    let pid = task.pid.0;
    trace!(
        "kernel:pid[{}] sys_munmap start={:#x} len={}",
        pid,
        _start,
        _len
    );
    if _len == 0 {
        return -1;
    }
    let start_va = VirtAddr::from(_start);
    if !start_va.aligned() {
        return -1;
    }
    let end = match _start.checked_add(_len) {
        Some(end) => end,
        None => return -1,
    };
    if end > TRAMPOLINE {
        return -1;
    }
    let end_va = VirtAddr::from(end);
    let start_vpn = start_va.floor();
    let end_vpn = end_va.ceil();
    if start_vpn >= end_vpn {
        return -1;
    }
    let mut inner = task.inner_exclusive_access();
    match inner.memory_set.area_end_vpn(start_vpn) {
        Some(existing_end) if existing_end == end_vpn => {
            inner.memory_set.remove_area_with_start_vpn(start_vpn);
            0
        }
        _ => -1,
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, _path);

    let data = if let Some(data) = get_app_data_by_name(path.as_str()) {
        data
    } else {
        // 程序未找到，spawn 失败
        return -1;
    };

    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;

    new_task.exec(data);

    add_task(new_task);
    new_pid as isize
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    let task = current_task().unwrap();
    let pid = task.pid.0;
    trace!("kernel:pid[{}] sys_set_priority {}", pid, prio);
    if prio < MIN_PRIORITY as isize {
        return -1;
    }
    {
        let mut inner = task.inner_exclusive_access();
        inner.set_priority(prio as usize);
    }
    prio
}
