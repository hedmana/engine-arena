//! Resident memory of a whole process group (e.g. `uv` plus its Python child).

#[cfg(target_os = "macos")]
pub fn group_rss(pgid: libc::pid_t) -> u64 {
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = [0 as libc::pid_t; 256];
    let size = std::mem::size_of_val(&pids) as libc::c_int;
    // SAFETY: the buffer is valid for `size` bytes; the call returns the number of bytes written.
    let bytes =
        unsafe { libc::proc_listpids(PROC_PGRP_ONLY, pgid as u32, pids.as_mut_ptr().cast(), size) };
    let count = (bytes.max(0) as usize / std::mem::size_of::<libc::pid_t>()).min(pids.len());
    pids[..count]
        .iter()
        .map(|&pid| {
            let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
            let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
            // SAFETY: `info` is valid for `size` bytes; it is only read if fully written.
            let written = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTASKINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            if written == size {
                unsafe { info.assume_init() }.pti_resident_size
            } else {
                0
            }
        })
        .sum()
}

#[cfg(target_os = "linux")]
pub fn group_rss(pgid: libc::pid_t) -> u64 {
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return 0;
    };
    entries
        .filter_map(|e| {
            let stat = std::fs::read_to_string(e.ok()?.path().join("stat")).ok()?;
            // Fields after the parenthesized command: state ppid pgrp ... rss is field 24 overall.
            let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
            (fields.get(2)?.parse::<libc::pid_t>().ok()? == pgid)
                .then(|| fields.get(21)?.parse::<u64>().ok())?
        })
        .sum::<u64>()
        * page
}
