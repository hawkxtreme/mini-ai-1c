//! Windows Job Object guard for automatic child process cleanup.
//!
//! When Mini AI 1C exits (including crashes), Windows automatically kills
//! all processes assigned to this Job Object via JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.
//!
//! Usage: call `assign_to_job(pid)` right after spawning any child process.

#[cfg(windows)]
mod inner {
    use std::sync::OnceLock;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW,
                JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, SetInformationJobObject,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };

    // Store HANDLE as usize (pointer cast) to satisfy Send + Sync requirements.
    static JOB: OnceLock<usize> = OnceLock::new();

    fn stderr_log(msg: &str) {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "{}", msg);
    }

    fn get_job() -> usize {
        *JOB.get_or_init(|| unsafe {
            let job = match CreateJobObjectW(None, None) {
                Ok(h) => h,
                Err(e) => {
                    stderr_log(&format!("[job_guard] CreateJobObjectW failed: {:?}", e));
                    return 0;
                }
            };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) {
                stderr_log(&format!("[job_guard] SetInformationJobObject failed: {:?}", e));
                let _ = CloseHandle(job);
                return 0;
            }
            let addr = job.0 as usize;
            stderr_log(&format!("[job_guard] Kill-on-close Job Object created (handle=0x{:x})", addr));
            addr
        })
    }

    pub fn assign(pid: u32) {
        let job_addr = get_job();
        if job_addr == 0 {
            return;
        }
        unsafe {
            let job = HANDLE(job_addr as *mut std::ffi::c_void);
            match OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
                Ok(proc) => {
                    if let Err(e) = AssignProcessToJobObject(job, proc) {
                        stderr_log(&format!("[job_guard] AssignProcessToJobObject(pid={}) failed: {:?}", pid, e));
                    } else {
                        stderr_log(&format!("[job_guard] pid={} assigned to kill-on-close job", pid));
                    }
                    let _ = CloseHandle(proc);
                }
                Err(e) => {
                    stderr_log(&format!("[job_guard] OpenProcess(pid={}) failed: {:?}", pid, e));
                }
            }
        }
    }
}

/// Assign a spawned child process to the global kill-on-close Job Object.
///
/// On Windows: if Mini AI 1C exits for any reason (including crash or kill),
/// the kernel automatically terminates all assigned child processes.
///
/// On non-Windows: no-op.
pub fn assign_to_job(pid: u32) {
    #[cfg(windows)]
    inner::assign(pid);

    #[cfg(not(windows))]
    let _ = pid;
}
