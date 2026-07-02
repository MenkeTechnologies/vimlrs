//! Ports of `src/nvim/os/` (platform layer). Only the pieces the eval engine
//! needs are mirrored here.

/// Port of `src/nvim/os/env.c` (subset: `os_get_pid`).
pub mod env;
/// Port of `src/nvim/os/fileio.c` + `os/fs.c` (the buffered `FileDescriptor`
/// and the `os_open`/`os_read`/`os_write`/… syscall leaves behind readfile/writefile).
pub mod fileio;
/// Port of `src/nvim/os/time.c` (subset: `os_hrtime`).
pub mod time;
