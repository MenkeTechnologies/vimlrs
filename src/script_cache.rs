//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. rkyv-backed bytecode cache for `.vim`
//! scripts, the same architecture as awkrs/strykelang/zshrs's `script_cache`.
//!
//! Single-file shard at `~/.cache/vimlrs/scripts.rkyv`. On the 2nd+ run of a
//! given script, lex/parse/compile is skipped — the cache hit is `mmap` +
//! zero-copy `ArchivedHashMap` lookup + a `bincode`-decode of the inner
//! `fusevm::Chunk` blob (the compiled program).
//!
//! Storage layout (rkyv archived):
//!   `ScriptShard { header: { magic, format_version, vimlrs_version,
//!                            pointer_width, built_at_secs },
//!                  entries: HashMap<canonical_path, ScriptEntry> }`
//!   `ScriptEntry { mtime_secs, mtime_nsecs, binary_mtime_at_cache,
//!                  cached_at_secs, chunk_blob }`
//!
//! Read path: lazy `mmap`, `rkyv::check_archived_root` validation, header
//! magic/version/pointer-width/vimlrs-version checks, then per-entry source
//! mtime + binary-mtime guards (any rebuild invalidates silently). Write path:
//! `flock(LOCK_EX)`, read-mutate-`to_bytes`, fsync to a `.tmp.<pid>.<nanos>`,
//! atomic-rename. The cache format is versioned from day one so an old cached
//! `.vim` that worked yesterday never breaks (compat-floor).
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use memmap2::Mmap;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

/// Magic header bytes — fail fast if a wrong-format file is mmap'd. ("VIML")
pub const SHARD_MAGIC: u32 = 0x56_49_4D_4C;
/// Bumped on incompatible rkyv schema changes (and when the meaning of emitted
/// builtin ids changes, e.g. the ignore-case comparison id remap).
pub const SHARD_FORMAT_VERSION: u32 = 3;

// ── rkyv archived types ──

/// Shard header: format identity + provenance.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ShardHeader {
    /// `SHARD_MAGIC`.
    pub magic: u32,
    /// `SHARD_FORMAT_VERSION`.
    pub format_version: u32,
    /// `CARGO_PKG_VERSION` of the writing binary.
    pub vimlrs_version: String,
    /// `size_of::<usize>()` of the writing binary.
    pub pointer_width: u32,
    /// Unix seconds the shard was last written.
    pub built_at_secs: u64,
}

/// One cached compiled script.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ScriptEntry {
    /// Source file mtime seconds.
    pub mtime_secs: i64,
    /// Source file mtime nanoseconds.
    pub mtime_nsecs: i64,
    /// `vimlrs` binary mtime when this entry was written.
    pub binary_mtime_at_cache: i64,
    /// Unix seconds the entry was written.
    pub cached_at_secs: i64,
    /// `bincode`-serialized `fusevm::Chunk`.
    pub chunk_blob: Vec<u8>,
}

/// The whole shard: header + path → entry map.
#[derive(Archive, RkyvDeserialize, RkyvSerialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct ScriptShard {
    /// Format identity.
    pub header: ShardHeader,
    /// Canonical-path → compiled-entry.
    pub entries: HashMap<String, ScriptEntry>,
}

// ── mmap'd validated shard view ──

/// mmap + validated `*const ArchivedScriptShard`. Self-referential — the
/// pointer is valid for the lifetime of the wrapping struct.
pub struct MmappedShard {
    _mmap: Mmap,
    archived: *const ArchivedScriptShard,
}

// SAFETY: the pointer aliases an immutable mmap that lives as long as Self;
// rkyv-validated reads are immutable.
unsafe impl Send for MmappedShard {}
unsafe impl Sync for MmappedShard {}

impl MmappedShard {
    /// mmap the shard file and validate its byte image.
    pub fn open(path: &Path) -> Option<Self> {
        let file = File::open(path).ok()?;
        let mmap = unsafe { Mmap::map(&file).ok()? };
        let archived = rkyv::check_archived_root::<ScriptShard>(&mmap[..]).ok()?;
        let archived = archived as *const ArchivedScriptShard;
        Some(Self {
            _mmap: mmap,
            archived,
        })
    }

    fn shard(&self) -> &ArchivedScriptShard {
        // SAFETY: see the Send/Sync impl comment.
        unsafe { &*self.archived }
    }

    fn header_ok(&self) -> bool {
        let h = &self.shard().header;
        let magic: u32 = h.magic.into();
        let fv: u32 = h.format_version.into();
        let pw: u32 = h.pointer_width.into();
        magic == SHARD_MAGIC
            && fv == SHARD_FORMAT_VERSION
            && pw as usize == std::mem::size_of::<usize>()
            && h.vimlrs_version.as_str() == env!("CARGO_PKG_VERSION")
    }

    fn lookup(&self, path: &str) -> Option<&ArchivedScriptEntry> {
        self.shard().entries.get(path)
    }
}

// ── ScriptCache: per-instance handle ──

/// A handle to one shard file plus its writer lock.
pub struct ScriptCache {
    path: PathBuf,
    lock_path: PathBuf,
    mmap: Mutex<Option<MmappedShard>>,
}

impl ScriptCache {
    /// Open (create the parent dir for) a shard at `path`.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("/tmp"));
        let lock_path = parent.join(format!(
            "{}.lock",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("scripts.rkyv")
        ));
        Ok(Self {
            path: path.to_path_buf(),
            lock_path,
            mmap: Mutex::new(None),
        })
    }

    fn ensure_mmap(&self) {
        let mut guard = self.mmap.lock().unwrap();
        if guard.is_none() {
            *guard = MmappedShard::open(&self.path);
        }
    }

    fn invalidate_mmap(&self) {
        *self.mmap.lock().unwrap() = None;
    }

    /// Cache lookup: returns the `bincode` blob on hit, `None` on miss, mtime
    /// mismatch, version drift, or `vimlrs`-binary newer than the cached entry.
    pub fn get(&self, path: &str, mtime_secs: i64, mtime_nsecs: i64) -> Option<Vec<u8>> {
        self.ensure_mmap();
        let guard = self.mmap.lock().unwrap();
        let shard = guard.as_ref()?;
        if !shard.header_ok() {
            return None;
        }
        let entry = shard.lookup(path)?;
        let entry_mtime_s: i64 = entry.mtime_secs.into();
        let entry_mtime_ns: i64 = entry.mtime_nsecs.into();
        if entry_mtime_s != mtime_secs || entry_mtime_ns != mtime_nsecs {
            return None;
        }
        if let Some(bin_mtime) = current_binary_mtime_secs() {
            let cached_bin_mtime: i64 = entry.binary_mtime_at_cache.into();
            if cached_bin_mtime < bin_mtime {
                return None;
            }
        }
        Some(entry.chunk_blob.as_slice().to_vec())
    }

    /// Insert / replace an entry. Serializes the whole shard and atomic-renames.
    pub fn put(
        &self,
        path: &str,
        mtime_secs: i64,
        mtime_nsecs: i64,
        chunk_blob: Vec<u8>,
    ) -> std::io::Result<()> {
        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return Ok(()),
        };
        let mut shard = match read_owned_shard(&self.path) {
            Some(s)
                if s.header.vimlrs_version == env!("CARGO_PKG_VERSION")
                    && s.header.pointer_width as usize == std::mem::size_of::<usize>()
                    && s.header.format_version == SHARD_FORMAT_VERSION =>
            {
                s
            }
            _ => fresh_shard(),
        };
        let bin_mtime = current_binary_mtime_secs().unwrap_or(0);
        shard.entries.insert(
            path.to_string(),
            ScriptEntry {
                mtime_secs,
                mtime_nsecs,
                binary_mtime_at_cache: bin_mtime,
                cached_at_secs: now_secs(),
                chunk_blob,
            },
        );
        shard.header.built_at_secs = now_secs() as u64;
        write_shard_atomic(&self.path, &shard)?;
        self.invalidate_mmap();
        Ok(())
    }

    /// Drop entries whose source file vanished or whose mtime changed.
    pub fn evict_stale(&self) -> usize {
        let _lock = match acquire_lock(&self.lock_path) {
            Some(l) => l,
            None => return 0,
        };
        let mut shard = match read_owned_shard(&self.path) {
            Some(s) => s,
            None => return 0,
        };
        let before = shard.entries.len();
        shard.entries.retain(|p, e| match file_mtime(Path::new(p)) {
            Some((s, ns)) => s == e.mtime_secs && ns == e.mtime_nsecs,
            None => false,
        });
        let evicted = before - shard.entries.len();
        if evicted > 0 {
            let _ = write_shard_atomic(&self.path, &shard);
            self.invalidate_mmap();
        }
        evicted
    }

    /// Delete the shard file. Idempotent.
    pub fn clear(&self) -> std::io::Result<()> {
        let _lock = acquire_lock(&self.lock_path);
        let res = match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        self.invalidate_mmap();
        res
    }
}

// ── locking + shard read/write helpers ──

fn acquire_lock(path: &Path) -> Option<nix::fcntl::Flock<File>> {
    let f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    nix::fcntl::Flock::lock(f, nix::fcntl::FlockArg::LockExclusive).ok()
}

fn fresh_shard() -> ScriptShard {
    ScriptShard {
        header: ShardHeader {
            magic: SHARD_MAGIC,
            format_version: SHARD_FORMAT_VERSION,
            vimlrs_version: env!("CARGO_PKG_VERSION").to_string(),
            pointer_width: std::mem::size_of::<usize>() as u32,
            built_at_secs: now_secs() as u64,
        },
        entries: HashMap::new(),
    }
}

fn read_owned_shard(path: &Path) -> Option<ScriptShard> {
    let bytes = std::fs::read(path).ok()?;
    let archived = rkyv::check_archived_root::<ScriptShard>(&bytes[..]).ok()?;
    archived.deserialize(&mut rkyv::Infallible).ok()
}

fn write_shard_atomic(path: &Path, shard: &ScriptShard) -> std::io::Result<()> {
    let bytes = rkyv::to_bytes::<_, 4096>(shard)
        .map_err(|e| std::io::Error::other(format!("rkyv serialize: {e}")))?;
    let parent = path.parent().expect("cache path has parent");
    let _ = std::fs::create_dir_all(parent);
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(
        "{}.tmp.{pid}.{nanos}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("scripts.rkyv")
    ));
    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Source-file mtime as `(secs, nsecs)`.
pub fn file_mtime(path: &Path) -> Option<(i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.mtime(), meta.mtime_nsec()))
}

/// mtime of the running `vimlrs` binary; cached for the process lifetime.
fn current_binary_mtime_secs() -> Option<i64> {
    static BIN_MTIME: OnceLock<Option<i64>> = OnceLock::new();
    *BIN_MTIME.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let (secs, _) = file_mtime(&exe)?;
        Some(secs)
    })
}

/// Default shard path: `~/.cache/vimlrs/scripts.rkyv`.
pub fn default_cache_path() -> PathBuf {
    dirs::cache_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("vimlrs/scripts.rkyv")
}

/// `VIMLRS_CACHE=0|false|no` disables the cache entirely.
pub fn cache_enabled() -> bool {
    !matches!(
        std::env::var("VIMLRS_CACHE").as_deref(),
        Ok("0") | Ok("false") | Ok("no")
    )
}

/// Process-wide cache rooted at [`default_cache_path`]; `None` when disabled or
/// unopenable.
pub static CACHE: once_cell::sync::Lazy<Option<ScriptCache>> = once_cell::sync::Lazy::new(|| {
    if !cache_enabled() {
        return None;
    }
    ScriptCache::open(&default_cache_path()).ok()
});

/// Try to load a cached [`CompiledProgram`](crate::compile_viml::CompiledProgram)
/// (the `main` chunk plus any user functions) for `path`.
pub fn try_load(path: &Path) -> Option<crate::compile_viml::CompiledProgram> {
    let cache = CACHE.as_ref()?;
    let canonical = path.canonicalize().ok()?;
    let key = canonical.to_str()?;
    let (mtime_s, mtime_ns) = file_mtime(&canonical)?;
    let blob = cache.get(key, mtime_s, mtime_ns)?;
    bincode::deserialize::<crate::compile_viml::CompiledProgram>(&blob).ok()
}

/// Store a compiled program for `path` (best-effort; errors ignored).
pub fn store(path: &Path, prog: &crate::compile_viml::CompiledProgram) {
    let Some(cache) = CACHE.as_ref() else {
        return;
    };
    let Ok(canonical) = path.canonicalize() else {
        return;
    };
    let Some(key) = canonical.to_str() else {
        return;
    };
    let Some((mtime_s, mtime_ns)) = file_mtime(&canonical) else {
        return;
    };
    let Ok(blob) = bincode::serialize(prog) else {
        return;
    };
    let _ = cache.put(key, mtime_s, mtime_ns, blob);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_roundtrip_via_rkyv() {
        let mut shard = fresh_shard();
        shard.entries.insert(
            "/tmp/x.vim".to_string(),
            ScriptEntry {
                mtime_secs: 1,
                mtime_nsecs: 2,
                binary_mtime_at_cache: 3,
                cached_at_secs: 4,
                chunk_blob: vec![9, 9, 9],
            },
        );
        let bytes = rkyv::to_bytes::<_, 4096>(&shard).unwrap();
        let archived = rkyv::check_archived_root::<ScriptShard>(&bytes[..]).unwrap();
        let back: ScriptShard = archived.deserialize(&mut rkyv::Infallible).unwrap();
        assert_eq!(back.entries["/tmp/x.vim"].chunk_blob, vec![9, 9, 9]);
        assert_eq!(back.header.magic, SHARD_MAGIC);
    }
}
