//! The plugin's only Mac-specific code: walking a process's parents. A port to another system fills in this module and nothing else.

// Too few ancestors and the pane is never placed.
const ANCESTOR_LIMIT: usize = 6;

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

pub fn ancestor_pids(start: u32) -> Vec<u32> {
    let mut pids = Vec::new();
    let mut current = start;
    while pids.len() < ANCESTOR_LIMIT && current > 1 {
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
    fn starts_at_the_given_pid_and_stops_at_the_limit_or_pid_1() {
        let own = std::process::id();

        let chain = ancestor_pids(own);
        assert_eq!(chain[0], own);
        assert!(chain.len() <= ANCESTOR_LIMIT);
        assert!(chain.iter().all(|pid| *pid > 1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn walks_up_to_the_parent_process() {
        let own = std::process::id();
        let parent = std::os::unix::process::parent_id();

        let chain = ancestor_pids(own);
        assert!(chain.len() > 1);
        assert_eq!(chain[1], parent);
    }
}
