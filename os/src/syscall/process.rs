//! Process management syscalls
use crate::mm::translated_byte_buffer;
use crate::syscall::syscall_id_to_name;
use crate::task::{change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
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
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    
    // current task page token
    let token = current_user_token();
    
    // handles  TimeVal crosses pages
    let buffers = translated_byte_buffer(
        token,
        ts as *const u8,
        core::mem::size_of::<TimeVal>()
    );
    
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    
    let time_bytes = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>()
        )
    };
    

    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&time_bytes[offset..offset + len]);
        offset += len;
    }
    
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace request={} id={} data={}", trace_request, id, data);
    
    match trace_request {
        // Read a byte from user memory at address `id`
        0 => {
            // Get current task's page table token
            let token = current_user_token();
            
            // Translate virtual address to physical address buffer
            let buffers = translated_byte_buffer(
                token,
                id as *const u8,
                1  // Read 1 byte
            );
            
            // Read the byte value
            if let Some(buffer) = buffers.first() {
                buffer[0] as isize
            } else {
                -1  // Translation failed
            }
        }
        
        // Write a byte to user memory at address `id`
        1 => {
            // Get current task's page table token
            let token = current_user_token();
            
            // Translate virtual address to physical address buffer
            let mut buffers = translated_byte_buffer(
                token,
                id as *const u8,
                1  // Write 1 byte
            );
            
            // Write the byte value (only lowest 8 bits)
            if let Some(buffer) = buffers.first_mut() {
                buffer[0] = (data & 0xFF) as u8;
                0  // Success
            } else {
                -1  // Translation failed
            }
        }
        
        // Query the number of times syscall `id` has been called
        2 => {
            // This would require syscall counter in TaskControlBlock
            // For now, just print the syscall name and return -1
            println!("[kernel] Query syscall: {} ({})", syscall_id_to_name(id), id);
            -1  // Not implemented yet (need to add syscall counter to TCB)
        }
        
        // Invalid trace_request
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    -1
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
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
