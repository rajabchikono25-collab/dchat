//! WASI Shim for DPL Programs
//!
//! Provides a minimal, deterministic subset of WASI for DPL contracts targeting
//! wasm32-wasi. This shim implements only the functions needed for basic Rust
//! stdlib support without introducing non-determinism.
//!
//! # Allowed Functions
//!
//! - `fd_write` - Write to stdout/stderr (logged, not actual I/O)
//! - `proc_exit` - Exit the program
//! - `environ_sizes_get` - Returns 0 environment variables
//! - `environ_get` - No-op (no environment)
//! - `args_sizes_get` - Returns 0 arguments
//! - `args_get` - No-op (no arguments)
//! - `fd_prestat_get` - Returns EBADF (no preopened dirs)
//! - `fd_prestat_dir_name` - Returns EBADF
//! - `fd_close` - Returns success for stdout/stderr, EBADF otherwise
//!
//! # Forbidden Functions (will fail validation)
//!
//! - `clock_time_get` - Non-deterministic (time)
//! - `random_get` - Non-deterministic (RNG)
//! - `fd_read` - No file I/O
//! - `path_*` - No filesystem access
//! - `sock_*` - No network access

use crate::vm::VmState;
use wasmi::{Caller, Linker};

/// WASI error codes
pub mod wasi_errno {
    /// Success
    pub const SUCCESS: u32 = 0;
    /// Bad file descriptor
    pub const EBADF: u32 = 8;
    /// Invalid argument
    pub const EINVAL: u32 = 28;
    /// Function not supported
    pub const ENOSYS: u32 = 52;
}

/// File descriptors
pub mod wasi_fd {
    /// Standard input
    pub const STDIN: u32 = 0;
    /// Standard output
    pub const STDOUT: u32 = 1;
    /// Standard error
    pub const STDERR: u32 = 2;
}

/// List of allowed WASI imports (with wasi: prefix for validation)
pub const ALLOWED_WASI_IMPORTS: &[&str] = &[
    "wasi:fd_write",
    "wasi:proc_exit",
    "wasi:environ_sizes_get",
    "wasi:environ_get",
    "wasi:args_sizes_get",
    "wasi:args_get",
    "wasi:fd_prestat_get",
    "wasi:fd_prestat_dir_name",
    "wasi:fd_close",
];

/// List of forbidden WASI imports (non-deterministic)
pub const FORBIDDEN_WASI_IMPORTS: &[&str] = &[
    "clock_time_get",
    "clock_res_get",
    "random_get",
    "fd_read",
    "fd_seek",
    "fd_tell",
    "fd_fdstat_get",
    "fd_fdstat_set_flags",
    "fd_fdstat_set_rights",
    "fd_filestat_get",
    "fd_filestat_set_size",
    "fd_filestat_set_times",
    "fd_pread",
    "fd_pwrite",
    "fd_readdir",
    "fd_renumber",
    "fd_sync",
    "fd_datasync",
    "fd_allocate",
    "fd_advise",
    "path_open",
    "path_create_directory",
    "path_remove_directory",
    "path_readlink",
    "path_rename",
    "path_filestat_get",
    "path_filestat_set_times",
    "path_link",
    "path_symlink",
    "path_unlink_file",
    "poll_oneoff",
    "sched_yield",
    "sock_recv",
    "sock_send",
    "sock_shutdown",
    "sock_accept",
];

/// Check if a WASI import function name is in the allowed list.
pub fn is_allowed_wasi_import(name: &str) -> bool {
    // Check both with and without wasi: prefix
    let with_prefix = format!("wasi:{}", name);
    ALLOWED_WASI_IMPORTS
        .iter()
        .any(|&s| s == name || s == with_prefix || s.ends_with(&format!(":{}", name)))
}

/// Check if a WASI import function name is in the forbidden list.
pub fn is_forbidden_wasi_import(name: &str) -> bool {
    FORBIDDEN_WASI_IMPORTS.contains(&name)
}

/// Register WASI shim functions with the linker.
///
/// This adds the minimal WASI preview1 functions needed for DPL programs
/// to run. All functions are deterministic stubs.
pub fn register_wasi_shim(linker: &mut Linker<VmState>) -> Result<(), wasmi::Error> {
    let module = "wasi_snapshot_preview1";

    // fd_write: write to stdout/stderr
    linker.func_wrap(
        module,
        "fd_write",
        |mut caller: Caller<'_, VmState>,
         fd: u32,
         iovs_ptr: u32,
         iovs_len: u32,
         nwritten_ptr: u32|
         -> u32 {
            // Only allow stdout (1) and stderr (2)
            if fd != wasi_fd::STDOUT && fd != wasi_fd::STDERR {
                return wasi_errno::EBADF;
            }

            let memory = match caller.get_export("memory") {
                Some(wasmi::Extern::Memory(mem)) => mem,
                _ => return wasi_errno::EINVAL,
            };

            let mut total_written: u32 = 0;

            // Process each iov (each is 8 bytes: ptr u32, len u32)
            for i in 0..iovs_len {
                let iov_offset = iovs_ptr as usize + (i as usize * 8);

                // Read iov_base and iov_len
                let mut buf = [0u8; 8];
                if memory.read(&caller, iov_offset, &mut buf).is_err() {
                    return wasi_errno::EINVAL;
                }

                let iov_base = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                let iov_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

                // Read the actual data (for logging)
                let mut data = vec![0u8; iov_len as usize];
                if memory.read(&caller, iov_base as usize, &mut data).is_err() {
                    return wasi_errno::EINVAL;
                }

                // In a real implementation, this would log to the transaction log
                // For now, we just count bytes as "written"
                total_written += iov_len;
            }

            // Write number of bytes written
            if memory
                .write(
                    &mut caller,
                    nwritten_ptr as usize,
                    &total_written.to_le_bytes(),
                )
                .is_err()
            {
                return wasi_errno::EINVAL;
            }

            wasi_errno::SUCCESS
        },
    )?;

    // proc_exit: exit the program
    linker.func_wrap(
        module,
        "proc_exit",
        |mut caller: Caller<'_, VmState>, code: u32| {
            // Set the error code in VM state and let the VM handle termination
            caller.data_mut().error_code = Some(code);
            // Return normally - the VM will check error_code after execution
        },
    )?;

    // environ_sizes_get: returns 0 environment variables
    linker.func_wrap(
        module,
        "environ_sizes_get",
        |mut caller: Caller<'_, VmState>,
         environ_count_ptr: u32,
         environ_buf_size_ptr: u32|
         -> u32 {
            let memory = match caller.get_export("memory") {
                Some(wasmi::Extern::Memory(mem)) => mem,
                _ => return wasi_errno::EINVAL,
            };

            // No environment variables
            if memory
                .write(&mut caller, environ_count_ptr as usize, &0u32.to_le_bytes())
                .is_err()
            {
                return wasi_errno::EINVAL;
            }
            if memory
                .write(
                    &mut caller,
                    environ_buf_size_ptr as usize,
                    &0u32.to_le_bytes(),
                )
                .is_err()
            {
                return wasi_errno::EINVAL;
            }

            wasi_errno::SUCCESS
        },
    )?;

    // environ_get: no-op (no environment)
    linker.func_wrap(
        module,
        "environ_get",
        |_caller: Caller<'_, VmState>, _environ: u32, _environ_buf: u32| -> u32 {
            wasi_errno::SUCCESS
        },
    )?;

    // args_sizes_get: returns 0 arguments
    linker.func_wrap(
        module,
        "args_sizes_get",
        |mut caller: Caller<'_, VmState>, argc_ptr: u32, argv_buf_size_ptr: u32| -> u32 {
            let memory = match caller.get_export("memory") {
                Some(wasmi::Extern::Memory(mem)) => mem,
                _ => return wasi_errno::EINVAL,
            };

            // No arguments
            if memory
                .write(&mut caller, argc_ptr as usize, &0u32.to_le_bytes())
                .is_err()
            {
                return wasi_errno::EINVAL;
            }
            if memory
                .write(&mut caller, argv_buf_size_ptr as usize, &0u32.to_le_bytes())
                .is_err()
            {
                return wasi_errno::EINVAL;
            }

            wasi_errno::SUCCESS
        },
    )?;

    // args_get: no-op (no arguments)
    linker.func_wrap(
        module,
        "args_get",
        |_caller: Caller<'_, VmState>, _argv: u32, _argv_buf: u32| -> u32 { wasi_errno::SUCCESS },
    )?;

    // fd_prestat_get: returns EBADF (no preopened directories)
    linker.func_wrap(
        module,
        "fd_prestat_get",
        |_caller: Caller<'_, VmState>, _fd: u32, _prestat_ptr: u32| -> u32 { wasi_errno::EBADF },
    )?;

    // fd_prestat_dir_name: returns EBADF
    linker.func_wrap(
        module,
        "fd_prestat_dir_name",
        |_caller: Caller<'_, VmState>, _fd: u32, _path: u32, _path_len: u32| -> u32 {
            wasi_errno::EBADF
        },
    )?;

    // fd_close: success for stdout/stderr, EBADF otherwise
    linker.func_wrap(
        module,
        "fd_close",
        |_caller: Caller<'_, VmState>, fd: u32| -> u32 {
            if fd == wasi_fd::STDOUT || fd == wasi_fd::STDERR {
                wasi_errno::SUCCESS
            } else {
                wasi_errno::EBADF
            }
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_imports_have_wasi_prefix() {
        for import in ALLOWED_WASI_IMPORTS {
            assert!(
                import.starts_with("wasi:"),
                "Missing wasi: prefix: {}",
                import
            );
        }
    }

    #[test]
    fn test_forbidden_imports_no_overlap() {
        for allowed in ALLOWED_WASI_IMPORTS {
            let func_name = allowed.strip_prefix("wasi:").unwrap();
            assert!(
                !FORBIDDEN_WASI_IMPORTS.contains(&func_name),
                "Overlap between allowed and forbidden: {}",
                func_name
            );
        }
    }
}
