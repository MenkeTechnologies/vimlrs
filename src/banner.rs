//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. VIMLRS ASCII wordmark + live-stats
//! box banner. Single source of truth shared by:
//!   - the interactive REPL startup (`repl::run`)
//!   - the `vimlrs --help` header (`cli::banner`, which reuses [`LOGO`])
//!
//! Every count is pulled from the reflection tables in [`crate::builtin_docs`]
//! at call time, so the banner never goes stale after a build adds builtins /
//! ex-commands / options. ANSI colors are toggled by the `colored` flag.
//! Adapted from strykelang's `banner.rs`.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::builtin_docs::{BUILTIN_CHAPTERS, BUILTIN_DOCS, EX_COMMANDS, OPTION_DOCS, V_VARS};

/// ANSI-Shadow "VIMLRS" wordmark (6 rows). The single source of truth for the
/// glyphs — both this banner and the `vimlrs --help` header in `cli.rs` read
/// these rows so the logo art is never duplicated. Renderers add one leading
/// space; the intra-art alignment spaces on rows 5–6 are part of the strings.
pub const LOGO: [&str; 6] = [
    "██╗   ██╗██╗███╗   ███╗██╗     ██████╗ ███████╗",
    "██║   ██║██║████╗ ████║██║     ██╔══██╗██╔════╝",
    "██║   ██║██║██╔████╔██║██║     ██████╔╝███████╗",
    "╚██╗ ██╔╝██║██║╚██╔╝██║██║     ██╔══██╗╚════██║",
    " ╚████╔╝ ██║██║ ╚═╝ ██║███████╗██║  ██║███████║",
    "  ╚═══╝  ╚═╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝",
];

/// Count of visible columns in `s`, ignoring ANSI SGR escape sequences.
/// Multi-byte UTF-8 is counted as one column per char — sufficient for the
/// box-drawing glyphs and Latin labels in the banner; East-Asian-Wide chars
/// would need a wcwidth-style lookup that we deliberately skip.
pub fn visible_width(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut w = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                i += 1;
            }
            i += 1;
        } else {
            let step = std::str::from_utf8(&bytes[i..])
                .ok()
                .and_then(|s| s.chars().next())
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            w += 1;
            i += step;
        }
    }
    w
}

/// Render the VIMLRS wordmark + stats box + tagline into a string.
/// `colored=true` emits ANSI SGR escapes; `false` returns plain text.
pub fn render_banner(colored: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let n_builtins = BUILTIN_DOCS.len();
    let n_ex = EX_COMMANDS.len();
    let n_vvars = V_VARS.len();
    let n_options = OPTION_DOCS.len();
    let n_chapters = BUILTIN_CHAPTERS.len();

    let (mem_total_gib, mem_avail_gib) = {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let avail = sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        (total, avail)
    };

    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let pid = std::process::id();

    let (c, m, r, y, g, n) = if colored {
        (
            "\x1b[36m", "\x1b[35m", "\x1b[31m", "\x1b[33m", "\x1b[32m", "\x1b[0m",
        )
    } else {
        ("", "", "", "", "", "")
    };

    const INNER: usize = 64;
    let bar = "─".repeat(INNER);
    let mut out = String::with_capacity(2048);

    // One interior box row: pad the visible body out to INNER, wrap in borders.
    let row = |out: &mut String, body: &str| {
        let pad = INNER.saturating_sub(visible_width(body));
        out.push_str(&format!("{c} │{n}{body}{:pad$}{c}│{n}\n", "", pad = pad));
    };

    // Wordmark: cyan / cyan / magenta / magenta / red / red (matches --help).
    out.push_str(&format!("{c} {}{n}\n", LOGO[0]));
    out.push_str(&format!("{c} {}{n}\n", LOGO[1]));
    out.push_str(&format!("{m} {}{n}\n", LOGO[2]));
    out.push_str(&format!("{m} {}{n}\n", LOGO[3]));
    out.push_str(&format!("{r} {}{n}\n", LOGO[4]));
    out.push_str(&format!("{r} {}{n}\n", LOGO[5]));

    out.push_str(&format!("{c} ┌{bar}┐{n}\n"));
    row(
        &mut out,
        &format!(
            " {y}SYSTEM{n}  status:{g} ONLINE {c}//{n} {y}os:{n} {os} {y}arch:{n} {arch} {y}pid:{n} {pid}"
        ),
    );
    row(
        &mut out,
        &format!(
            " {y}CORES{n}   {cores}    {y}MEM{n}  {mem_avail_gib:.1} {c}/{n} {mem_total_gib:.1} GiB available"
        ),
    );
    out.push_str(&format!("{c} ├{bar}┤{n}\n"));
    row(
        &mut out,
        &format!(
            " {y}builtins{n}  {n_builtins:<5} {y}ex-cmds{n}  {n_ex:<5} {y}v:vars{n}  {n_vvars:<5}"
        ),
    );
    row(
        &mut out,
        &format!(" {y}options{n}   {n_options:<5} {y}chapters{n} {n_chapters:<5}"),
    );
    out.push_str(&format!("{c} └{bar}┘{n}\n"));
    out.push_str(&format!(
        "{m}  >> VIML INTERPRETER ON FUSEVM // FULL SPECTRUM v{version} <<{n}\n"
    ));
    out
}

/// Print the banner to stdout. Convenience wrapper around [`render_banner`].
pub fn print_banner(colored: bool) {
    print!("{}", render_banner(colored));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_ignores_csi_sequences() {
        assert_eq!(visible_width("\x1b[31mabc\x1b[0m"), 3);
        assert_eq!(visible_width("\x1b[1;38;5;202mok"), 2);
    }

    #[test]
    fn visible_width_counts_each_char_once_for_multibyte() {
        // 3 box-drawing glyphs, each 3 bytes UTF-8, but one column each.
        assert_eq!(visible_width("─├┤"), 3);
        assert_eq!(visible_width("aé你"), 3);
    }

    #[test]
    fn visible_width_handles_empty_and_lone_escape() {
        assert_eq!(visible_width(""), 0);
        // Lone ESC with no `[` does not start a CSI; counts as 1 char.
        assert_eq!(visible_width("\x1bz"), 2);
    }

    #[test]
    fn render_banner_plain_has_no_ansi_escapes() {
        let s = render_banner(false);
        assert!(!s.contains('\x1b'), "plain banner must not contain ESC");
        assert!(s.contains("VIML INTERPRETER"));
        assert!(s.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn render_banner_colored_contains_ansi_escapes() {
        let s = render_banner(true);
        assert!(s.contains("\x1b["));
        assert!(s.contains("\x1b[0m"));
    }

    #[test]
    fn render_banner_rows_all_match_inner_width_after_strip() {
        // Anchor expected width to the top border, then prove every interior
        // row matches it. Catches drift in `row()` padding — and, crucially,
        // any stats body that overflows INNER (which would silently push the
        // right border off and fail this equality).
        let s = render_banner(false);
        let top = s
            .lines()
            .find(|l| l.starts_with(" ┌"))
            .expect("top border present");
        let want = visible_width(top);
        let mut box_rows = 0;
        for line in s.lines() {
            if line.starts_with(" │") && line.ends_with('│') {
                box_rows += 1;
                assert_eq!(
                    visible_width(line),
                    want,
                    "box row width drift on line: {line}"
                );
            }
        }
        assert!(box_rows >= 4, "expected several rendered box rows");
    }

    #[test]
    fn logo_has_six_rows_and_matches_help_glyphs() {
        assert_eq!(LOGO.len(), 6);
        // First row is the flush-left top of the ANSI-Shadow "V".
        assert!(LOGO[0].starts_with("██╗"));
    }
}
