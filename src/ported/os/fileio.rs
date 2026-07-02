//! Port of `src/nvim/os/fileio.c` (vendored at `csrc/os/fileio.c`).
//!
//! Buffered reading/writing to a file: an fopen/fread/fwrite replacement that
//! does not deal with Nvim buffer/autocommand structures. `writefile()` writes
//! through this `FileDescriptor` abstraction.
//!
//! RUST-PORT NOTE: the C struct stores `read_pos`/`write_pos` as raw pointers
//! into a heap `buffer`; Rust has no interior-pointer arithmetic, so they are
//! modelled as byte offsets (`usize`) into `buffer: Vec<u8>`. `alloc_block()` /
//! `free_block()` become `Vec` allocation / drop.
//!
//! RUST-PORT NOTE: the `os_*` leaf calls live in `os/fs.c` (vendored at
//! `csrc/os/fs.c`) where they wrap libuv's synchronous `uv_fs_*` API and the
//! raw `read`/`write` syscalls. libuv's synchronous fs calls map 1:1 to the
//! POSIX syscalls, so they are ported here against `nix::libc` (the same
//! os-layer adaptation as `os/time.rs`). `os_translate_sys_error(errno)` maps a
//! system errno `E` to the libuv code `-E` on unix, mirrored here by negating
//! `errno`.
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use nix::libc;

/// `#define ARENA_BLOCK_SIZE 4096` (`memory_defs.h:14`) — the read/write buffer
/// block size.
pub const ARENA_BLOCK_SIZE: usize = 4096;

// ── `FileOpenFlags` (fileio.h:9) ─────────────────────────────────────────────
/// Open file read-only. Default.
pub const kFileReadOnly: i32 = 1;
/// Create file if it does not exist yet. Implies kFileWriteOnly.
pub const kFileCreate: i32 = 2;
/// Open file for writing only. Cannot be used with kFileReadOnly.
pub const kFileWriteOnly: i32 = 4;
/// Do not allow symbolic links.
pub const kFileNoSymlink: i32 = 8;
/// Only create the file, failing if it already exists. Implies kFileWriteOnly.
pub const kFileCreateOnly: i32 = 16;
/// Truncate the file if it exists. Implies kFileWriteOnly.
pub const kFileTruncate: i32 = 32;
/// Append to the file. Implies kFileWriteOnly.
pub const kFileAppend: i32 = 64;
/// Do not restart read()/write() syscall if EAGAIN was encountered.
pub const kFileNonBlocking: i32 = 128;
/// Create parent directories as needed.
pub const kFileMkDir: i32 = 256;

// ── libuv error codes (`os_translate_sys_error(errno) == -errno` on unix) ─────
const UV_EAGAIN: i32 = -libc::EAGAIN;
const UV_EINTR: i32 = -libc::EINTR;
const UV_EIO: i32 = -libc::EIO;
const UV_EINVAL: i32 = -libc::EINVAL;
const UV_EROFS: i32 = -libc::EROFS;
const UV_ENOTSUP: i32 = -libc::ENOTSUP;
/// libuv's `UV_UNKNOWN` sentinel.
const UV_UNKNOWN: i32 = -4094;

/// Port of `os_translate_sys_error()` (`os/os_defs.h:52` → `uv_translate_sys_error`)
/// — map a system errno to the libuv code `-E`. RUST-PORT NOTE: the C form takes
/// `errno` as an argument; here the thread errno is read via `last_os_error()`.
fn os_translate_sys_error() -> i32 {
    -std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Port of `struct { … } FileDescriptor` from `csrc/os/fileio_defs.h`.
///
/// Structure used to read from/write to file. `read_pos`/`write_pos` are byte
/// offsets into `buffer` (see file-header RUST-PORT NOTE).
#[derive(Default)]
pub struct FileDescriptor {
    /// File descriptor. Can be -1 if no backing file (`file_open_buffer`).
    pub fd: i32,
    /// Read or write buffer. Always `ARENA_BLOCK_SIZE` if allocated.
    pub buffer: Vec<u8>,
    /// Read position in `buffer`.
    pub read_pos: usize,
    /// Write position in `buffer`.
    pub write_pos: usize,
    /// True if file is in write mode.
    pub wr: bool,
    /// True if end of file was encountered.
    pub eof: bool,
    /// True if EAGAIN should not restart syscalls.
    pub non_blocking: bool,
    /// Total bytes read so far.
    pub bytes_read: u64,
}

/// Port of `file_space()` from `csrc/os/fileio.h:36` — bytes free in the buffer.
fn file_space(fp: &FileDescriptor) -> usize {
    // c: (fp->buffer + ARENA_BLOCK_SIZE) - fp->write_pos
    ARENA_BLOCK_SIZE - fp.write_pos
}

/// Port of `file_open()` from `csrc/os/fileio.c:41`.
///
/// Open file. Returns an error code, or 0 on success (@see `os_strerror()`).
pub fn file_open(ret_fp: &mut FileDescriptor, fname: &str, flags: i32, mode: i32) -> i32 {
    let mut os_open_flags = 0i32;
    // c: FLAG(flags, flag, fcntl_flags, wrval, cond) accumulates fcntl flags for
    //    each FileOpenFlags bit set. RUST-PORT NOTE: the `wr` TriState and its
    //    `assert(cond)` guards are compile-time sanity checks with no runtime
    //    effect (`(void)wr`), so they are dropped.
    if flags & kFileWriteOnly != 0 {
        os_open_flags |= libc::O_WRONLY;
    }
    if flags & kFileCreateOnly != 0 {
        os_open_flags |= libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY;
    }
    if flags & kFileCreate != 0 {
        os_open_flags |= libc::O_CREAT | libc::O_WRONLY;
    }
    if flags & kFileTruncate != 0 {
        os_open_flags |= libc::O_TRUNC | libc::O_WRONLY;
    }
    if flags & kFileAppend != 0 {
        os_open_flags |= libc::O_APPEND | libc::O_WRONLY;
    }
    if flags & kFileReadOnly != 0 {
        os_open_flags |= libc::O_RDONLY;
    }
    // c: #ifdef O_NOFOLLOW … (present on the unix targets we support)
    if flags & kFileNoSymlink != 0 {
        os_open_flags |= libc::O_NOFOLLOW;
    }
    if flags & kFileMkDir != 0 {
        os_open_flags |= libc::O_CREAT | libc::O_WRONLY;
    }

    if flags & kFileMkDir != 0 {
        let mkdir_ret = os_file_mkdir(fname, 0o755);
        if mkdir_ret < 0 {
            return mkdir_ret;
        }
    }

    let fd = os_open(fname, os_open_flags, mode);

    if fd < 0 {
        return fd;
    }
    file_open_fd(ret_fp, fd, flags)
}

/// Port of `file_open_fd()` from `csrc/os/fileio.c:103` — wrap a file descriptor
/// with a `FileDescriptor`. Returns 0 (currently always).
pub fn file_open_fd(ret_fp: &mut FileDescriptor, fd: i32, flags: i32) -> i32 {
    ret_fp.wr = (flags
        & (kFileCreate | kFileCreateOnly | kFileTruncate | kFileAppend | kFileWriteOnly))
        != 0;
    ret_fp.non_blocking = (flags & kFileNonBlocking) != 0;
    // Non-blocking writes not supported currently.
    debug_assert!(!ret_fp.wr || !ret_fp.non_blocking);
    ret_fp.fd = fd;
    ret_fp.eof = false;
    ret_fp.buffer = alloc_block();
    ret_fp.read_pos = 0;
    ret_fp.write_pos = 0;
    ret_fp.bytes_read = 0;
    0
}

/// Port of `file_open_stdin()` from `csrc/os/fileio.c:124` — open standard input.
pub fn file_open_stdin(fp: &mut FileDescriptor) -> i32 {
    let error = file_open_fd(fp, os_open_stdin_fd(), kFileReadOnly | kFileNonBlocking);
    // c: if (error != 0) ELOG("failed to open stdin: %s", os_strerror(error));
    //    RUST-PORT NOTE: the ELOG diagnostic is dropped.
    error
}

/// Port of `file_open_buffer()` from `csrc/os/fileio.c:135` — open a buffer for
/// reading. RUST-PORT NOTE: C points `read_pos`/`write_pos` into the caller's
/// `data` (`buffer = NULL`, no ownership); with the offset model the bytes are
/// copied into an owned buffer.
pub fn file_open_buffer(ret_fp: &mut FileDescriptor, data: &[u8]) {
    ret_fp.wr = false;
    ret_fp.non_blocking = false;
    ret_fp.fd = -1;
    ret_fp.eof = true;
    ret_fp.buffer = data.to_vec();
    ret_fp.read_pos = 0;
    ret_fp.write_pos = data.len();
    ret_fp.bytes_read = 0;
}

/// Port of `file_close()` from `csrc/os/fileio.c:153` — close file and free its
/// buffer. Returns 0 or an error code.
pub fn file_close(fp: &mut FileDescriptor, do_fsync: bool) -> i32 {
    if fp.fd < 0 {
        return 0;
    }

    let flush_error = if do_fsync { file_fsync(fp) } else { file_flush(fp) };
    let close_error = os_close(fp.fd);
    free_block(&mut fp.buffer);
    if close_error != 0 {
        return close_error;
    }
    flush_error
}

/// Port of `file_fsync()` from `csrc/os/fileio.c:174` — flush modifications to
/// disk and run `fsync()`. Returns 0 or an error code.
pub fn file_fsync(fp: &mut FileDescriptor) -> i32 {
    if !fp.wr {
        return 0;
    }
    let flush_error = file_flush(fp);
    if flush_error != 0 {
        return flush_error;
    }
    let fsync_error = os_fsync(fp.fd);
    if fsync_error != UV_EINVAL
        && fsync_error != UV_EROFS
        // fsync not supported on this storage.
        && fsync_error != UV_ENOTSUP
    {
        return fsync_error;
    }
    0
}

/// Port of `file_flush()` from `csrc/os/fileio.c:199` — flush modifications to
/// disk. Returns 0 or an error code.
pub fn file_flush(fp: &mut FileDescriptor) -> i32 {
    if !fp.wr {
        return 0;
    }

    let to_write = (fp.write_pos - fp.read_pos) as isize;
    if to_write == 0 {
        return 0;
    }
    let wres = os_write(
        fp.fd,
        &fp.buffer[fp.read_pos..fp.write_pos],
        to_write as usize,
        fp.non_blocking,
    );
    fp.read_pos = 0;
    fp.write_pos = 0;
    if wres != to_write {
        return if wres >= 0 { UV_EIO } else { wres as i32 };
    }
    0
}

/// Port of `file_read()` from `csrc/os/fileio.c:227` — read from file. Returns
/// an error code (< 0) or the number of bytes read.
///
/// RUST-PORT NOTE: the `#else` (no `HAVE_READV`) branch is ported; the `readv`
/// combined buffer/RBuffer fast path is a platform alternative not taken.
pub fn file_read(fp: &mut FileDescriptor, ret_buf: &mut [u8], size: usize) -> isize {
    debug_assert!(!fp.wr);
    let from_buffer = std::cmp::min(fp.write_pos - fp.read_pos, size);
    ret_buf[..from_buffer].copy_from_slice(&fp.buffer[fp.read_pos..fp.read_pos + from_buffer]);

    let bufpos = from_buffer; // c: char *buf = ret_buf + from_buffer;
    let mut read_remaining = size - from_buffer;
    if read_remaining == 0 {
        fp.bytes_read += from_buffer as u64;
        fp.read_pos += from_buffer;
        return from_buffer as isize;
    }

    // at this point, we have consumed all of an existing buffer. restart from the beginning
    fp.read_pos = 0;
    fp.write_pos = 0;

    if fp.eof {
        // already eof, cannot read more
    } else if read_remaining >= ARENA_BLOCK_SIZE {
        // …otherwise leave fp->buffer empty and populate only target buffer,
        // because filtering information through rbuffer will be more syscalls.
        let r_ret = os_read(
            fp.fd,
            &mut fp.eof,
            &mut ret_buf[bufpos..bufpos + read_remaining],
            read_remaining,
            fp.non_blocking,
        );
        if r_ret >= 0 {
            read_remaining -= r_ret as usize;
        } else {
            return r_ret;
        }
    } else {
        let r_ret = os_read(
            fp.fd,
            &mut fp.eof,
            &mut fp.buffer[fp.write_pos..fp.write_pos + ARENA_BLOCK_SIZE],
            ARENA_BLOCK_SIZE,
            fp.non_blocking,
        );
        if r_ret < 0 {
            return r_ret;
        } else {
            fp.write_pos += r_ret as usize;
            let to_copy = std::cmp::min(r_ret as usize, read_remaining);
            ret_buf[bufpos..bufpos + to_copy]
                .copy_from_slice(&fp.buffer[fp.read_pos..fp.read_pos + to_copy]);
            fp.read_pos += to_copy;
            read_remaining -= to_copy;
        }
    }

    fp.bytes_read += (size - read_remaining) as u64;
    (size - read_remaining) as isize
}

/// Port of `file_write()` from `csrc/os/fileio.c:330` — write to a file. Returns
/// the number of bytes written or a libuv error code (< 0).
pub fn file_write(fp: &mut FileDescriptor, buf: &[u8], size: usize) -> isize {
    debug_assert!(fp.wr);
    // includes the trivial case of size==0
    if size < file_space(fp) {
        fp.buffer[fp.write_pos..fp.write_pos + size].copy_from_slice(&buf[..size]);
        fp.write_pos += size;
        return size as isize;
    }

    let status = file_flush(fp);
    if status < 0 {
        return status as isize;
    }

    if size < ARENA_BLOCK_SIZE {
        fp.buffer[fp.write_pos..fp.write_pos + size].copy_from_slice(&buf[..size]);
        fp.write_pos += size;
        return size as isize;
    }

    let wres = os_write(fp.fd, buf, size, fp.non_blocking);
    if wres != size as isize && wres >= 0 {
        UV_EIO as isize
    } else {
        wres
    }
}

// ── `os/fs.c` leaves (vendored at `csrc/os/fs.c`; see file-header note) ───────

/// Port of `os_open()` from `csrc/os/fs.c:420` — open a path, returning the fd
/// or a negative error code. C runs `uv_fs_open` synchronously → `open()`.
fn os_open(path: &str, flags: i32, mode: i32) -> i32 {
    // c: if (path == NULL) return UV_EINVAL;  (a &str carries no NULL; an
    //    interior-NUL path is rejected as EINVAL by CString::new.)
    let cpath = match std::ffi::CString::new(path) {
        Ok(c) => c,
        Err(_) => return UV_EINVAL,
    };
    let r = unsafe { libc::open(cpath.as_ptr(), flags, mode as libc::c_uint) };
    if r < 0 {
        os_translate_sys_error()
    } else {
        r
    }
}

/// Port of `os_close()` from `csrc/os/fs.c:527`.
fn os_close(fd: i32) -> i32 {
    let r = unsafe { libc::close(fd) };
    if r < 0 {
        os_translate_sys_error()
    } else {
        r
    }
}

/// Port of `os_dup()` from `csrc/os/fs.c:539`.
fn os_dup(fd: i32) -> i32 {
    // c: os_dup_dup: label — retry on EINTR.
    loop {
        let ret = unsafe { libc::dup(fd) };
        if ret < 0 {
            let error = os_translate_sys_error();
            if error == UV_EINTR {
                continue;
            } else {
                return error;
            }
        }
        return ret;
    }
}

/// Port of `os_open_stdin_fd()` from `csrc/os/fs.c:558`.
fn os_open_stdin_fd() -> i32 {
    // c: `stdin_fd` global (set for `--headless` etc.) is not present standalone,
    //    so `stdin_fd > 0` is false → dup STDIN_FILENO.
    os_dup(libc::STDIN_FILENO)
}

/// Port of `os_read()` from `csrc/os/fs.c:585` — read up to `size` bytes.
/// Returns the number of bytes read (0 with `*ret_eof` set at EOF), or a
/// negative error code.
fn os_read(fd: i32, ret_eof: &mut bool, ret_buf: &mut [u8], size: usize, non_blocking: bool) -> isize {
    *ret_eof = false;
    // c: if (ret_buf == NULL) { assert(size == 0); return 0; }  (a slice is never
    //    NULL; an empty request returns 0 via the loop guard.)
    let mut read_bytes = 0usize;
    while read_bytes != size {
        debug_assert!(size >= read_bytes);
        // c: IO_COUNT(size - read_bytes) is an identity cast on unix.
        let cur_read_bytes = unsafe {
            libc::read(
                fd,
                ret_buf[read_bytes..].as_mut_ptr() as *mut libc::c_void,
                size - read_bytes,
            )
        };
        if cur_read_bytes > 0 {
            read_bytes += cur_read_bytes as usize;
        }
        if cur_read_bytes < 0 {
            let error = os_translate_sys_error();
            if non_blocking && error == UV_EAGAIN {
                break;
            } else if error == UV_EINTR || error == UV_EAGAIN {
                continue;
            } else {
                return error as isize;
            }
        }
        if cur_read_bytes == 0 {
            *ret_eof = true;
            break;
        }
    }
    read_bytes as isize
}

/// Port of `os_write()` from `csrc/os/fs.c:690` — write `size` bytes. Returns
/// the number of bytes written or a negative error code.
fn os_write(fd: i32, buf: &[u8], size: usize, non_blocking: bool) -> isize {
    // c: if (buf == NULL) { assert(size == 0); return 0; }
    let mut written_bytes = 0usize;
    while written_bytes != size {
        debug_assert!(size >= written_bytes);
        let cur_written_bytes = unsafe {
            libc::write(
                fd,
                buf[written_bytes..].as_ptr() as *const libc::c_void,
                size - written_bytes,
            )
        };
        if cur_written_bytes > 0 {
            written_bytes += cur_written_bytes as usize;
        }
        if cur_written_bytes < 0 {
            let error = os_translate_sys_error();
            if non_blocking && error == UV_EAGAIN {
                break;
            } else if error == UV_EINTR || error == UV_EAGAIN {
                continue;
            } else {
                return error as isize;
            }
        }
        if cur_written_bytes == 0 {
            return UV_UNKNOWN as isize;
        }
    }
    written_bytes as isize
}

/// Port of `os_fsync()` from `csrc/os/fs.c:743`. RUST-PORT NOTE: the `g_stats`
/// counter bump has no counterpart standalone and is dropped.
fn os_fsync(fd: i32) -> i32 {
    let r = unsafe { libc::fsync(fd) };
    if r < 0 {
        os_translate_sys_error()
    } else {
        r
    }
}

/// Port of `os_file_mkdir()` from `csrc/os/fs.c:1080` — create the parent
/// directory of `fname` if it does not already exist. Returns 0 or a negative
/// error code.
///
/// RUST-PORT NOTE: the C body walks `path_tail_with_sep()`/`os_mkdir_recurse()`
/// (path.c); standalone this collapses to `create_dir_all()` on the parent.
fn os_file_mkdir(fname: &str, _mode: i32) -> i32 {
    let parent = match std::path::Path::new(fname).parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return 0,
    };
    if parent.exists() {
        return 0;
    }
    match std::fs::create_dir_all(parent) {
        Ok(()) => 0,
        Err(e) => -e.raw_os_error().unwrap_or(0),
    }
}

/// Port of `os_strerror()` (`os/os_defs.h:49` → `uv_strerror`) — a human string
/// for a (negative) libuv/errno error code.
pub fn os_strerror(error: i32) -> String {
    std::io::Error::from_raw_os_error(-error).to_string()
}

/// Port of `alloc_block()` from `Src/memory.c:708` — a fresh `ARENA_BLOCK_SIZE`
/// block. RUST-PORT NOTE: the C block cache (`arena_reuse_blk`) is an allocator
/// optimization with no observable effect and is dropped.
fn alloc_block() -> Vec<u8> {
    vec![0u8; ARENA_BLOCK_SIZE]
}

/// Port of `free_block()` from `Src/memory.c:778` — free a block.
fn free_block(block: &mut Vec<u8>) {
    *block = Vec::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_write_read_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir
            .join(format!("vimlrs_fileio_{}.tmp", std::process::id()))
            .to_string_lossy()
            .into_owned();

        let mut fp = FileDescriptor::default();
        let err = file_open(
            &mut fp,
            &path,
            kFileCreate | kFileTruncate | kFileWriteOnly,
            0o644,
        );
        assert_eq!(err, 0, "file_open: {}", os_strerror(err));
        assert!(fp.wr);
        let payload = b"hello\nworld";
        assert_eq!(file_write(&mut fp, payload, payload.len()), payload.len() as isize);
        assert_eq!(file_close(&mut fp, false), 0);
        assert!(fp.buffer.is_empty(), "buffer freed on close");

        // Read it back through file_read.
        let mut rp = FileDescriptor::default();
        assert_eq!(file_open(&mut rp, &path, kFileReadOnly, 0), 0);
        let mut buf = [0u8; 64];
        let n = file_read(&mut rp, &mut buf, payload.len());
        assert_eq!(n, payload.len() as isize);
        assert_eq!(&buf[..payload.len()], payload);
        assert_eq!(file_close(&mut rp, false), 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn large_write_flushes_through_syscall() {
        // A write larger than ARENA_BLOCK_SIZE goes straight to os_write.
        let dir = std::env::temp_dir();
        let path = dir
            .join(format!("vimlrs_fileio_big_{}.tmp", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let mut fp = FileDescriptor::default();
        assert_eq!(
            file_open(&mut fp, &path, kFileCreate | kFileTruncate | kFileWriteOnly, 0o644),
            0
        );
        let big = vec![7u8; ARENA_BLOCK_SIZE * 3 + 5];
        assert_eq!(file_write(&mut fp, &big, big.len()), big.len() as isize);
        assert_eq!(file_close(&mut fp, true), 0);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), big.len() as u64);
        let _ = std::fs::remove_file(&path);
    }
}
