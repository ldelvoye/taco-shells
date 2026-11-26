use std::ffi::{CStr, c_void};
use std::mem;
use std::ptr;

const MAX_GROUP: usize = 1024;
/// From `<libproc.h>`: list the processes of one process group.
const PROC_PGRP_ONLY: u32 = 2;
/// The length of a `KERN_PROCARGS2` sysctl name.
const PROCARGS_NAME_LENGTH: libc::c_uint = 3;

/// A process's working directory.
pub fn directory_of(pid: u32) -> Option<String> {
    let mut info: libc::proc_vnodepathinfo = unsafe { mem::zeroed() };
    let size = mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let pointer = &mut info as *mut libc::proc_vnodepathinfo as *mut c_void;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            pointer,
            size,
        )
    };
    if got != size {
        return None;
    }
    let path_pointer = info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char;
    let path = unsafe { CStr::from_ptr(path_pointer) };
    let text = path.to_str().ok()?;
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// The command lines of the processes in a process group, as `ps -o command=` shows them.
pub fn group_commands(group: u32) -> Vec<String> {
    let mut pids = vec![0 as libc::c_int; MAX_GROUP];
    let capacity = (pids.len() * mem::size_of::<libc::c_int>()) as libc::c_int;
    let buffer = pids.as_mut_ptr() as *mut c_void;
    let bytes = unsafe { libc::proc_listpids(PROC_PGRP_ONLY, group, buffer, capacity) };
    if bytes <= 0 {
        return Vec::new();
    }
    let count = bytes as usize / mem::size_of::<libc::c_int>();

    let mut commands = Vec::new();
    for pid in &pids[..count] {
        if *pid <= 0 {
            continue;
        }
        if let Some(command) = command_line(*pid) {
            commands.push(command);
        }
    }
    commands
}

/// A process's arguments joined by spaces, from `KERN_PROCARGS2`.
fn command_line(pid: libc::c_int) -> Option<String> {
    let mut size: libc::size_t = 0;
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let asked = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            PROCARGS_NAME_LENGTH,
            ptr::null_mut(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if asked != 0 || size < mem::size_of::<libc::c_int>() {
        return None;
    }
    let mut buffer = vec![0u8; size];
    let pointer = buffer.as_mut_ptr() as *mut c_void;
    let read = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            PROCARGS_NAME_LENGTH,
            pointer,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buffer.truncate(size);

    let count_bytes: [u8; 4] = buffer[..4].try_into().ok()?;
    let argument_count = i32::from_ne_bytes(count_bytes) as usize;
    let rest = &buffer[4..];
    // The argument count, then the executable's path, padding, and the arguments.
    let path_end = rest.iter().position(|byte| *byte == 0)?;
    let after_path = &rest[path_end..];
    let arguments_start = after_path.iter().position(|byte| *byte != 0)?;
    let arguments = &after_path[arguments_start..];

    let mut words = Vec::new();
    for word in arguments.split(|byte| *byte == 0).take(argument_count) {
        let text = String::from_utf8_lossy(word);
        words.push(text.into_owned());
    }
    Some(words.join(" "))
}
