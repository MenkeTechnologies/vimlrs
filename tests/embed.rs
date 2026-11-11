//! `vimlrs::run_program`: per-call interpreter, embedded stdin, captured stdout.

use vimlrs::run_program;

#[test]
fn echo_output_is_returned() {
    assert_eq!(
        run_program("echo 'hello'\necho 1 + 2", "").unwrap(),
        "hello\n3\n"
    );
}

/// `input()` reads the embedded stdin a line at a time; its prompt is output too.
/// EOF cancels, returning the default.
#[test]
fn input_reads_the_given_stdin() {
    let src = "let a = input('a? ')\nlet b = input('b? ')\nlet c = input('c? ', 'none')\necho toupper(a) b c";
    assert_eq!(
        run_program(src, "abc\nxyz\n").unwrap(),
        "a? b? c? ABC xyz none\n"
    );
}

#[test]
fn errors_are_returned() {
    assert!(run_program("echo (1", "").is_err());
    let err = run_program("echo 'before'\ncall NoSuchFunc()", "").unwrap_err();
    assert!(err.contains("E117"), "{err}");
    // A caught error is not a failure.
    let src = "try\ncall NoSuchFunc()\ncatch\necho 'caught'\nendtry";
    assert_eq!(run_program(src, "").unwrap(), "caught\n");
}

/// Each call starts from a fresh interpreter: a global set by one run is unset in the next.
#[test]
fn calls_do_not_share_state() {
    run_program("let g:leak = 1", "").unwrap();
    assert_eq!(run_program("echo exists('g:leak')", "").unwrap(), "0\n");
}

#[test]
fn concurrent_calls_keep_their_own_output() {
    let runs: Vec<_> = (0..8)
        .map(|n| {
            std::thread::spawn(move || {
                let src = format!("for i in range(200)\necho {n}\nendfor");
                (n, run_program(&src, "").unwrap())
            })
        })
        .collect();
    for run in runs {
        let (n, out) = run.join().unwrap();
        assert_eq!(out, format!("{n}\n").repeat(200));
    }
}
