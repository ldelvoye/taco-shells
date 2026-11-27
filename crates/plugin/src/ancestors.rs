// Too few ancestors and the pane is never placed.
pub const ANCESTOR_LIMIT: usize = 6;

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

// Only macOS walks to the parent here; elsewhere the chain stops at the start pid.
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

        let several = ancestor_pids(own, ANCESTOR_LIMIT);
        assert_eq!(several[0], own);
        assert!(!several.contains(&0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn walks_up_to_the_parent_process() {
        let own = std::process::id();
        let parent = std::os::unix::process::parent_id();

        let chain = ancestor_pids(own, ANCESTOR_LIMIT);
        assert!(chain.len() > 1);
        assert_eq!(chain[1], parent);
    }
}
