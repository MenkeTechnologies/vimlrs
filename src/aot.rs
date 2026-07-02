//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. Ahead-of-time build: bake one or more
//! `.vim` scripts into a copy of the running `vimlrs` binary as a
//! zstd-compressed trailer, producing a self-contained executable. At startup
//! `vimlrs` detects the trailer and runs every embedded script in input order
//! as one program. Same design as zshrs's `aot.rs` (which has no C counterpart
//! either — Vim's `.vim` scripts have no compile-into-binary form).
//!
//! Trailer layout (little-endian, appended to a copy of the `vimlrs` binary):
//!
//! ```text
//!   [elf/mach-o bytes of vimlrs ...]   (unchanged, still runs as `vimlrs`)
//!   [zstd-compressed payload ...]
//!   [u64 compressed_len][u64 uncompressed_len][u32 version][u32 reserved]
//!   [8 bytes magic  b"VIMLRAOT"]
//! ```
//!
//! Payload v2 (ordered file list): `[u32 count]` then per file
//! `[u32 name_len][name][u32 src_len][source]`.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Trailer magic (8 bytes).
pub const AOT_MAGIC: &[u8; 8] = b"VIMLRAOT";
/// Trailer format version 1: single script (legacy decode-only).
pub const AOT_VERSION_V1: u32 = 1;
/// Trailer format version 2: ordered file list (current build output).
pub const AOT_VERSION_V2: u32 = 2;
/// Fixed trailer length: `8 (cl) + 8 (ul) + 4 (ver) + 4 (rsv) + 8 (magic)`.
pub const TRAILER_LEN: u64 = 32;

/// One embedded script.
#[derive(Debug, Clone)]
pub struct EmbeddedFile {
    /// Error-reporting name (e.g. `hello.vim`).
    pub name: String,
    /// UTF-8 VimL source.
    pub source: String,
}

/// One or more embedded files, in build order.
#[derive(Debug, Clone)]
pub struct EmbeddedFiles(pub Vec<EmbeddedFile>);

fn encode_payload_v2(files: &[EmbeddedFile]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        64 + files
            .iter()
            .map(|f| f.name.len() + f.source.len() + 8)
            .sum::<usize>(),
    );
    let count = u32::try_from(files.len()).expect("file count fits in u32");
    out.extend_from_slice(&count.to_le_bytes());
    for f in files {
        let name_len = u32::try_from(f.name.len()).expect("name length fits in u32");
        let src_len = u32::try_from(f.source.len()).expect("source length fits in u32");
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(f.name.as_bytes());
        out.extend_from_slice(&src_len.to_le_bytes());
        out.extend_from_slice(f.source.as_bytes());
    }
    out
}

fn decode_payload_v2(bytes: &[u8]) -> Option<EmbeddedFiles> {
    let mut pos = 0usize;
    if bytes.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
    pos += 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if pos + 4 > bytes.len() {
            return None;
        }
        let name_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if pos + name_len > bytes.len() {
            return None;
        }
        let name = std::str::from_utf8(&bytes[pos..pos + name_len])
            .ok()?
            .to_string();
        pos += name_len;
        if pos + 4 > bytes.len() {
            return None;
        }
        let src_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        pos += 4;
        if pos + src_len > bytes.len() {
            return None;
        }
        let source = std::str::from_utf8(&bytes[pos..pos + src_len])
            .ok()?
            .to_string();
        pos += src_len;
        out.push(EmbeddedFile { name, source });
    }
    Some(EmbeddedFiles(out))
}

/// v1 decoder kept for backward compat: a one-script payload promoted into a
/// single-element [`EmbeddedFiles`].
fn decode_payload_v1(bytes: &[u8]) -> Option<EmbeddedFiles> {
    if bytes.len() < 4 {
        return None;
    }
    let name_len = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    if 4 + name_len > bytes.len() {
        return None;
    }
    let name = std::str::from_utf8(&bytes[4..4 + name_len])
        .ok()?
        .to_string();
    let source = std::str::from_utf8(&bytes[4 + name_len..])
        .ok()?
        .to_string();
    Some(EmbeddedFiles(vec![EmbeddedFile { name, source }]))
}

fn build_trailer(compressed_len: u64, uncompressed_len: u64, version: u32) -> [u8; 32] {
    let mut trailer = [0u8; 32];
    trailer[0..8].copy_from_slice(&compressed_len.to_le_bytes());
    trailer[8..16].copy_from_slice(&uncompressed_len.to_le_bytes());
    trailer[16..20].copy_from_slice(&version.to_le_bytes());
    // 20..24 reserved (zeros).
    trailer[24..32].copy_from_slice(AOT_MAGIC);
    trailer
}

/// Append a compressed v2 ordered-file payload to an existing file.
pub fn append_embedded_files(out_path: &Path, files: &[EmbeddedFile]) -> io::Result<()> {
    let payload = encode_payload_v2(files);
    let compressed = zstd::stream::encode_all(&payload[..], 3)?;
    let mut f = OpenOptions::new().append(true).open(out_path)?;
    f.write_all(&compressed)?;
    let trailer = build_trailer(
        compressed.len() as u64,
        payload.len() as u64,
        AOT_VERSION_V2,
    );
    f.write_all(&trailer)?;
    f.sync_all()?;
    Ok(())
}

/// Fast probe: read the last 32 bytes of `exe` and return embedded files in
/// build order if present (decodes v1 and v2). Called at `vimlrs` startup.
pub fn try_load_embedded(exe: &Path) -> Option<EmbeddedFiles> {
    let mut f = File::open(exe).ok()?;
    let size = f.metadata().ok()?.len();
    if size < TRAILER_LEN {
        return None;
    }
    f.seek(SeekFrom::End(-(TRAILER_LEN as i64))).ok()?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    f.read_exact(&mut trailer).ok()?;
    if &trailer[24..32] != AOT_MAGIC {
        return None;
    }
    let compressed_len = u64::from_le_bytes(trailer[0..8].try_into().ok()?);
    let uncompressed_len = u64::from_le_bytes(trailer[8..16].try_into().ok()?);
    let version = u32::from_le_bytes(trailer[16..20].try_into().ok()?);
    if compressed_len == 0 || compressed_len > size - TRAILER_LEN {
        return None;
    }
    let payload_start = size - TRAILER_LEN - compressed_len;
    f.seek(SeekFrom::Start(payload_start)).ok()?;
    let mut compressed = vec![0u8; compressed_len as usize];
    f.read_exact(&mut compressed).ok()?;
    let payload = zstd::stream::decode_all(&compressed[..]).ok()?;
    if payload.len() != uncompressed_len as usize {
        return None;
    }
    match version {
        AOT_VERSION_V1 => decode_payload_v1(&payload),
        AOT_VERSION_V2 => decode_payload_v2(&payload),
        _ => None,
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut p = meta.permissions();
        p.set_mode(p.mode() | 0o111);
        let _ = fs::set_permissions(path, p);
    }
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

/// Copy `src` to `dst`, skipping any existing AOT trailer (prevents nested
/// builds from stacking trailers).
fn copy_exe_without_trailer(src: &Path, dst: &Path) -> io::Result<()> {
    let mut sf = File::open(src)?;
    let size = sf.metadata()?.len();
    let keep = if size >= TRAILER_LEN {
        sf.seek(SeekFrom::End(-(TRAILER_LEN as i64)))?;
        let mut trailer = [0u8; TRAILER_LEN as usize];
        if sf.read_exact(&mut trailer).is_ok() && &trailer[24..32] == AOT_MAGIC {
            let compressed_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
            if compressed_len > 0 && compressed_len <= size - TRAILER_LEN {
                size - TRAILER_LEN - compressed_len
            } else {
                size
            }
        } else {
            size
        }
    } else {
        size
    };
    sf.seek(SeekFrom::Start(0))?;
    let _ = fs::remove_file(dst);
    let mut df = File::create(dst)?;
    let mut remaining = keep;
    let mut buf = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let n = std::cmp::min(remaining as usize, buf.len());
        sf.read_exact(&mut buf[..n])?;
        df.write_all(&buf[..n])?;
        remaining -= n as u64;
    }
    df.sync_all()?;
    Ok(())
}

/// `vimlrs --build OUT A.vim B.vim`: bake A and B into a copy of the running
/// `vimlrs` binary in input order, producing a self-contained AOT executable.
pub fn build(script_paths: &[PathBuf], out_path: &Path) -> Result<PathBuf, String> {
    if script_paths.is_empty() {
        return Err("vimlrs --build: at least one script path required".to_string());
    }
    let mut files: Vec<EmbeddedFile> = Vec::with_capacity(script_paths.len());
    for p in script_paths {
        let source = fs::read_to_string(p)
            .map_err(|e| format!("vimlrs --build: cannot read {}: {e}", p.display()))?;
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script.vim")
            .to_string();
        files.push(EmbeddedFile { name, source });
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("vimlrs --build: locating current executable: {e}"))?;
    copy_exe_without_trailer(&exe, out_path).map_err(|e| {
        format!(
            "vimlrs --build: copy {} -> {}: {e}",
            exe.display(),
            out_path.display()
        )
    })?;
    append_embedded_files(out_path, &files)
        .map_err(|e| format!("vimlrs --build: write trailer: {e}"))?;
    set_executable(out_path);
    Ok(out_path.to_path_buf())
}

// ───────────────────────── Native AOT (`--build --native`) ─────────────────
//
// Unlike the source-trailer build above (which embeds the script text and
// re-runs it interpreted at startup), the native path compiles the script to a
// fusevm chunk, lowers it to native machine code via `fusevm::aot` (Cranelift
// `ObjectModule` → relocatable `.o`), and links the object against the VimL
// runtime staticlib (`libvimlrs.a`) into a standalone executable.

/// Frontend runtime hook invoked by `fusevm::aot::fusevm_aot_run_embedded` at
/// startup of a native AOT binary: install the VimL host + builtins on the run
/// VM (the same setup the interpreter's `run_chunk` does via `install`).
///
/// # Safety
/// `vm` is the live run VM passed by the fusevm runtime; borrowed only here.
#[no_mangle]
// FFI entry point registered with the AOT runtime by raw `extern "C"` fn
// pointer; the `# Safety` contract above governs the deref. Marking the fn
// `unsafe` would change its type and break registration.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn fusevm_aot_register_builtins(vm: *mut fusevm::VM) {
    // SAFETY: the fusevm runtime hands us the live run VM for this call.
    let vm = unsafe { &mut *vm };
    crate::fusevm_bridge::install(vm);
}

/// Locate the VimL runtime staticlib to link against. `VIMLRS_AOT_RUNTIME_LIB`
/// overrides; otherwise look for `libvimlrs.a` beside the running executable.
fn runtime_staticlib() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("VIMLRS_AOT_RUNTIME_LIB") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(dir) = exe.parent() {
        let cand = dir.join("libvimlrs.a");
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err("could not locate libvimlrs.a (set VIMLRS_AOT_RUNTIME_LIB)".to_string())
}

/// `vimlrs --build OUT --native A.vim [B.vim]`: AOT-compile the inputs to native
/// machine code and link a standalone executable. Inputs are concatenated in
/// order into one program (matching the source-trailer build).
pub fn build_native(script_paths: &[PathBuf], out_path: &Path) -> Result<PathBuf, String> {
    if script_paths.is_empty() {
        return Err("vimlrs --build --native: at least one script path required".to_string());
    }
    let mut source = String::new();
    for p in script_paths {
        let s = fs::read_to_string(p)
            .map_err(|e| format!("vimlrs --build --native: cannot read {}: {e}", p.display()))?;
        source.push_str(&s);
        if !source.ends_with('\n') {
            source.push('\n');
        }
    }
    let stmts = crate::viml_parser::parse_program(&source)
        .map_err(|e| format!("vimlrs --build --native: {e}"))?;
    let prog = crate::compile_viml::compile_program(&stmts)
        .map_err(|e| format!("vimlrs --build --native: {e}"))?;
    if !prog.funcs.is_empty() {
        return Err(
            "vimlrs --build --native: scripts defining `:function` are not yet supported \
             (user functions compile to a separate registry, not the main chunk); \
             use `--build` without `--native` for those"
                .to_string(),
        );
    }
    if prog.main.ops.is_empty() {
        return Err("vimlrs --build --native: script compiled to an empty chunk".to_string());
    }

    let runtime_lib = runtime_staticlib()?;
    if !runtime_lib.exists() {
        return Err(format!(
            "vimlrs --build --native: runtime staticlib not found at {}",
            runtime_lib.display()
        ));
    }

    let obj = out_path.with_extension("o");
    fusevm::aot::compile_object(&prog.main, &obj)
        .map_err(|e| format!("vimlrs --build --native: {e}"))?;

    let stub = out_path.with_extension("aot_main.c");
    fs::write(
        &stub,
        b"extern long fusevm_aot_run_embedded(void);\nint main(void){return (int)fusevm_aot_run_embedded();}\n" as &[u8],
    )
    .map_err(|e| format!("vimlrs --build --native: write entry stub: {e}"))?;

    let mut cmd = std::process::Command::new("cc");
    cmd.arg(&stub).arg(&obj).arg(&runtime_lib);
    if cfg!(target_os = "macos") {
        // chrono/iana-time-zone pull CoreFoundation on macOS.
        cmd.arg("-framework").arg("CoreFoundation");
    }
    cmd.arg("-o").arg(out_path);
    let status = cmd
        .status()
        .map_err(|e| format!("vimlrs --build --native: invoking cc: {e}"))?;
    let _ = fs::remove_file(&stub);
    let _ = fs::remove_file(&obj);
    if !status.success() {
        return Err(format!(
            "vimlrs --build --native: link failed (cc exit {:?})",
            status.code()
        ));
    }
    set_executable(out_path);
    Ok(out_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_layout_matches_spec() {
        let t = build_trailer(0x11_22_33_44, 0xAA_BB_CC_DD, AOT_VERSION_V2);
        assert_eq!(t.len(), TRAILER_LEN as usize);
        assert_eq!(&t[0..8], &0x11_22_33_44u64.to_le_bytes());
        assert_eq!(&t[8..16], &0xAA_BB_CC_DDu64.to_le_bytes());
        assert_eq!(&t[16..20], &AOT_VERSION_V2.to_le_bytes());
        assert_eq!(&t[20..24], &[0u8; 4]);
        assert_eq!(&t[24..32], AOT_MAGIC);
    }

    #[test]
    fn payload_v2_roundtrip_preserves_order() {
        let files = vec![
            EmbeddedFile {
                name: "a.vim".into(),
                source: "echo 1\n".into(),
            },
            EmbeddedFile {
                name: "b.vim".into(),
                source: "echo 2\n".into(),
            },
        ];
        let decoded = decode_payload_v2(&encode_payload_v2(&files)).unwrap();
        assert_eq!(decoded.0.len(), 2);
        assert_eq!(decoded.0[0].name, "a.vim");
        assert_eq!(decoded.0[1].source, "echo 2\n");
    }
}
