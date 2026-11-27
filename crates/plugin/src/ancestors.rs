#[cfg(target_os = "macos")]
fn parent_of(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let pointer = &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void;
    let got =
        unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, pointer, size) };
    if got != size {
        return None;
    }
    Some(info.pbi_ppid)
}

#[cfg(not(target_os = "macos"))]
fn parent_of(_pid: u32) -> Option<u32> {
    None
}

pub fn ancestor_pids(start: u32, limit: usize) -> Vec<u32> {
    let mut pids = Vec::new();
    let mut current = start;
    while pids.len() < limit && current > 1 {
        pids.push(current);
        let Some(parent) = parent_of(current) else {
            break;
        };
        current = parent;
    }
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_given_pid_and_respects_the_limit() {
        let own = std::process::id();

        let one = ancestor_pids(own, 1);
        assert_eq!(one, [own]);

        let several = ancestor_pids(own, 6);
        assert_eq!(several[0], own);
        assert!(several.len() <= 6);
        assert!(!several.contains(&0));
    }
}
