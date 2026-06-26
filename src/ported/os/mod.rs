//! Ports of `src/nvim/os/` (platform layer). Only the pieces the eval engine
//! needs are mirrored here.

/// Port of `src/nvim/os/time.c` (subset: `os_hrtime`).
pub mod time;
