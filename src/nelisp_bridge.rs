// Phase 1.C.2 — embedded NeLisp session bridge.
//
// `Session` wraps a persistent `Env' so successive `eval' calls share
// state — this is what `(setq load-path ...)' followed by `(require
// '...)' on a later turn relies on, mirroring how the canonical
// `bin/nemacs' bash launcher hands the same boot form to the `nelisp'
// driver.  Each GUI process keeps a single Session alive for its
// lifetime.  Future phases will grow this module into the full
// `nelisp-display-*' plug-in surface that elisp Layer 2 calls into for
// frame creation, key event injection, and per-glyph face lookup.

use nelisp::eval::{eval, Env, EvalError};
use nelisp::reader::{read_str, ReadError, Sexp};

/// Wrapper for the two error kinds we surface from `eval_form` so
/// callers can map both to the same printable string.
#[derive(Debug)]
pub enum BridgeError {
    Read(ReadError),
    Eval(EvalError),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Read(e) => write!(f, "read: {}", e),
            BridgeError::Eval(e) => write!(f, "{}", e),
        }
    }
}

/// Long-lived NeLisp evaluation session backed by a single global `Env'.
pub struct Session {
    env: Env,
}

impl Session {
    pub fn new() -> Self {
        Self { env: Env::new_global() }
    }

    /// Read + eval `form_str' against the session's persistent env.
    pub fn eval_form(&mut self, form_str: &str) -> Result<Sexp, BridgeError> {
        let form = read_str(form_str).map_err(BridgeError::Read)?;
        eval(&form, &mut self.env).map_err(BridgeError::Eval)
    }

    /// Convenience: eval and stringify the result (or the error).
    pub fn eval_to_string(&mut self, form_str: &str) -> String {
        match self.eval_form(form_str) {
            Ok(s) => format!("{}", s),
            Err(e) => format!("ERR {}", e),
        }
    }
}

/// Resolve the Layer 2 elisp source root.  Search order:
///   1. `NEMACS_HOME` env var     → `$NEMACS_HOME/src`
///   2. `NEMACS_LAYER2_SRC` env var (= explicit override)
///   3. Walk a list of well-known dev candidates, preferring those that
///      contain `nelisp-emacs-compat.el' (= the marker file shipped only
///      by the active substrate worktree, missing from the older
///      canonical clone).  Both the direct (`Cowork/Notes/...') and
///      symlinked (`Notes/...') prefixes are tried so the resolver works
///      regardless of which prefix the user's process can stat.
///   4. Canonical fallback.
pub fn layer2_src_path() -> String {
    if let Ok(home) = std::env::var("NEMACS_HOME") {
        return format!("{}/src", home.trim_end_matches('/'));
    }
    if let Ok(p) = std::env::var("NEMACS_LAYER2_SRC") {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let candidates = [
        // Direct + symlinked worktree paths.
        format!(
            "{home}/Cowork/Notes/dev/nelisp-emacs/.worktrees/emacs-builtins-port/src"
        ),
        format!(
            "{home}/Notes/dev/nelisp-emacs/.worktrees/emacs-builtins-port/src"
        ),
        // Direct + symlinked canonical clone paths.
        format!("{home}/Cowork/Notes/dev/nelisp-emacs/src"),
        format!("{home}/Notes/dev/nelisp-emacs/src"),
    ];
    for c in &candidates {
        if std::path::Path::new(c)
            .join("nelisp-emacs-compat.el")
            .exists()
        {
            return c.clone();
        }
    }
    // Fallback: canonical clone (symlink form), kept stable for users who
    // haven't yet checked out the latest substrate.
    candidates[3].clone()
}

/// Resolve the NEMACS_HOME-equivalent root (= parent of `src/').
/// Used to derive `nelisp-emacs-vendor-root' (= `<root>/vendor') which
/// `emacs-init.el' reads to extend `load-path' with the upstream Emacs
/// elisp tree.
pub fn layer2_home_path() -> String {
    let src = layer2_src_path();
    if let Some(stripped) = src.strip_suffix("/src") {
        stripped.to_string()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{}/Notes/dev/nelisp-emacs", home)
    }
}

/// Boot form that primes a fresh Session with the Layer 2 load-path
/// + `nelisp-emacs-vendor-root' + `(require 'emacs-init)' which is the
/// canonical master require chain pulling in every sibling
/// substrate module in the correct order (= `bin/nemacs' bash launcher
/// does the equivalent before `(nemacs-main)').
///
/// Idempotent — safe to run multiple times against the same Session.
pub fn layer2_setup_form() -> String {
    let src = layer2_src_path();
    let home = layer2_home_path();
    format!(
        r#"(progn
            (unless (boundp 'load-path) (defvar load-path nil))
            (unless (member "{src}" load-path)
              (setq load-path (cons "{src}" load-path)))
            (unless (boundp 'nelisp-emacs-vendor-root)
              (defvar nelisp-emacs-vendor-root nil))
            (setq nelisp-emacs-vendor-root "{home}/vendor")
            (require 'emacs-init)
            'load-path-ready)"#,
    )
}

/// Phase 1.D.3b — install a minimal global keymap so the substrate's
/// `emacs-command-loop-step' can resolve our GTK-derived events.
///
/// Mirrors the subset of `nemacs-main--init-keymap' (`nemacs-main.el')
/// that the GUI session actually fires:
///
/// - ASCII 32..126 → `self-insert-command'
/// - byte 13      → `newline'
/// - byte 127     → `delete-backward-char'
/// - `'backspace'  → `delete-backward-char'
/// - `'left'       → `backward-char'
/// - `'right'      → `forward-char'
/// - `'up'         → `previous-line'
/// - `'down'       → `next-line'
///
/// Idempotent — calling this twice replaces the previous global map
/// with a fresh sparse keymap of the same shape.  Layer 2 must already
/// be loaded (= run `layer2_setup_form' first); this form depends on
/// the unprefixed aliases provided by `emacs-keymap-builtins' /
/// `emacs-command-loop-builtins' that `(require 'emacs-init)' chains.
pub fn command_loop_setup_form() -> &'static str {
    r#"(progn
        (let ((m (make-sparse-keymap)))
          (let ((c 32))
            (while (<= c 126)
              (define-key m (vector c) 'self-insert-command)
              (setq c (1+ c))))
          (define-key m (vector 13) 'newline)
          (define-key m (vector 'return) 'newline)
          (define-key m (vector 'backspace) 'delete-backward-char)
          (define-key m (vector 127) 'delete-backward-char)
          (define-key m (vector 'left) 'backward-char)
          (define-key m (vector 'right) 'forward-char)
          (define-key m (vector 'up) 'previous-line)
          (define-key m (vector 'down) 'next-line)
          (use-global-map m))
        'keymap-ready)"#
}

/// Build the elisp form that feeds one event into the substrate
/// command-loop and runs a single dispatch step against the named
/// buffer.  EVENT_LITERAL is rendered verbatim into the form: an
/// integer literal (= "65"), a quoted symbol (= "'backspace"), or any
/// other reader-acceptable event shape.
///
/// The whole step runs inside `with-current-buffer (get-buffer ...)`
/// so:
///   1. keymap lookup walks the buffer's local map first (= future
///      mode-specific bindings),
///   2. the executed command (= `self-insert-command' / `newline' /
///      …) sees the right `current-buffer' and edits in place.
pub fn command_loop_dispatch_form(buffer: &str, event_literal: &str) -> String {
    format!(
        r#"(with-current-buffer (get-buffer "{buffer}")
            (emacs-command-loop-feed-events {event_literal})
            (emacs-command-loop-step))"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_arithmetic() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(+ 1 2)"), "3");
    }

    #[test]
    fn session_persists_setq_across_calls() {
        let mut s = Session::new();
        s.eval_form("(setq foo 41)").unwrap();
        assert_eq!(s.eval_to_string("(+ foo 1)"), "42");
    }

    #[test]
    fn multiplication() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(* 7 8)"), "56");
    }

    #[test]
    fn string_concat() {
        let mut s = Session::new();
        assert_eq!(
            s.eval_to_string("(concat \"hello, \" \"world\")"),
            "\"hello, world\""
        );
    }

    #[test]
    fn list_length() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(length '(a b c d e))"), "5");
    }

    #[test]
    fn nested_call() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(+ (* 3 4) (* 5 6))"), "42");
    }

    #[test]
    fn car_cdr() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(car (cdr '(a b c)))"), "b");
    }

    #[test]
    fn error_form_returns_err_prefix() {
        let mut s = Session::new();
        let r = s.eval_to_string("(this-symbol-does-not-exist)");
        assert!(
            r.starts_with("ERR "),
            "expected ERR-prefixed error, got {r:?}"
        );
    }

    #[test]
    fn layer2_src_path_default_under_home() {
        // Sanity check that the default path resolution always returns
        // a non-empty string ending in `/src'.  (We don't assert the
        // file exists here — that's a Layer 2 deployment concern.)
        let p = layer2_src_path();
        assert!(p.ends_with("/src"), "{p:?} should end with /src");
    }

    #[test]
    fn debug_layer2_path_resolution() {
        // Print resolved path + check what files exist there.  Only
        // useful as a diagnostic when we suspect path resolution is
        // wrong — `cargo test debug_layer2_path_resolution -- --nocapture'
        // shows the values.
        let p = layer2_src_path();
        eprintln!("layer2_src_path = {p}");
        for f in [
            "emacs-error.el",
            "emacs-buffer-builtins.el",
            "nelisp-emacs-compat.el",
            "cl-lib.el",
            "nelisp-regex.el",
            "nelisp-text-buffer.el",
        ] {
            let path = std::path::Path::new(&p).join(f);
            eprintln!("  {} {}", if path.exists() { "OK" } else { "MISSING" }, f);
        }
    }

    #[test]
    fn welcome_buffer_creation_round_trip() {
        let mut s = Session::new();
        s.eval_form(&layer2_setup_form()).unwrap();
        let r = s.eval_to_string(
            r#"(progn
                (require 'emacs-buffer-builtins)
                (let ((buf (or (get-buffer "*welcome*")
                               (generate-new-buffer "*welcome*"))))
                  (with-current-buffer buf
                    (erase-buffer)
                    (insert "hello\n")
                    (insert "world\n")
                    (buffer-string))))"#,
        );
        eprintln!("welcome_buffer => {r}");
        assert!(
            !r.starts_with("ERR "),
            "welcome buffer creation failed: {r}"
        );
        // Result should be the printed form of "hello\nworld\n" (= with
        // outer quotes + decoded \n).
        assert!(r.contains("hello"));
        assert!(r.contains("world"));
    }

    /// Helper: bring a Session to a state where *welcome* exists and
    /// the global keymap is wired for the command-loop tests below.
    fn boot_with_welcome_and_keymap(s: &mut Session) {
        let setup = s.eval_to_string(&layer2_setup_form());
        assert!(!setup.starts_with("ERR "), "layer2 setup failed: {setup}");
        let buf = s.eval_to_string(
            r#"(progn
                (require 'emacs-buffer-builtins)
                (let ((b (or (get-buffer "*welcome*")
                             (generate-new-buffer "*welcome*"))))
                  (with-current-buffer b
                    (erase-buffer)
                    (insert "> "))
                  (buffer-name b)))"#,
        );
        assert!(!buf.starts_with("ERR "), "welcome setup failed: {buf}");
        let km = s.eval_to_string(command_loop_setup_form());
        assert!(!km.starts_with("ERR "), "keymap setup failed: {km}");
    }

    #[test]
    fn command_loop_setup_form_returns_keymap_ready() {
        let mut s = Session::new();
        boot_with_welcome_and_keymap(&mut s);
        // Re-running the setup form should still return `keymap-ready'
        // (= idempotent / second use-global-map replaces the first).
        let r = s.eval_to_string(command_loop_setup_form());
        assert_eq!(r, "keymap-ready");
    }

    #[test]
    fn command_loop_dispatch_self_insert_round_trip() {
        let mut s = Session::new();
        boot_with_welcome_and_keymap(&mut s);
        // Feed 'X' (= integer 88) → `self-insert-command' → buffer ends
        // with "> X".
        let r = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "88"));
        assert!(!r.starts_with("ERR "), "dispatch failed: {r}");
        let buf = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        assert!(buf.contains('X'), "expected X in buffer; got {buf}");
    }

    #[test]
    fn command_loop_dispatch_backspace_round_trip() {
        let mut s = Session::new();
        boot_with_welcome_and_keymap(&mut s);
        // Insert two chars via command-loop, then backspace one.
        let _ = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "65"));
        let _ = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "66"));
        let buf1 = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        assert!(buf1.contains("AB"), "expected AB after inserts: {buf1}");

        let r = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "'backspace"));
        assert!(!r.starts_with("ERR "), "backspace dispatch failed: {r}");
        let buf2 = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        assert!(!buf2.contains("AB"), "B should be deleted: {buf2}");
        assert!(buf2.contains('A'), "A should survive: {buf2}");
    }

    #[test]
    fn command_loop_dispatch_arrow_motion() {
        let mut s = Session::new();
        boot_with_welcome_and_keymap(&mut s);
        // Insert "abc" → point ends at 6 (= "> abc|"), then Left twice
        // moves point to 4.
        for code in [97, 98, 99] {
            let r = s.eval_to_string(&command_loop_dispatch_form(
                "*welcome*",
                &code.to_string(),
            ));
            assert!(!r.starts_with("ERR "), "insert {code} failed: {r}");
        }
        let p1 = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (point))"#,
        );
        assert_eq!(p1.trim(), "6", "expected point=6 after 3 inserts; got {p1}");

        for _ in 0..2 {
            let r = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "'left"));
            assert!(!r.starts_with("ERR "), "left dispatch failed: {r}");
        }
        let p2 = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (point))"#,
        );
        assert_eq!(p2.trim(), "4", "expected point=4 after 2 lefts; got {p2}");
    }

    #[test]
    fn command_loop_dispatch_return_inserts_newline() {
        let mut s = Session::new();
        boot_with_welcome_and_keymap(&mut s);
        let r = s.eval_to_string(&command_loop_dispatch_form("*welcome*", "13"));
        assert!(!r.starts_with("ERR "), "return dispatch failed: {r}");
        let buf = s.eval_to_string(
            r#"(with-current-buffer (get-buffer "*welcome*") (buffer-string))"#,
        );
        // Buffer was "> "; after newline it should contain a `\n'.
        assert!(buf.contains("\\n"), "expected newline in buffer; got {buf}");
    }

    #[test]
    fn require_emacs_buffer_builtins_through_session() {
        // Reproduces the GUI bootstrap failure outside the display loop.
        let mut s = Session::new();
        let setup = s.eval_to_string(&layer2_setup_form());
        eprintln!("setup => {setup}");
        let lp = s.eval_to_string("(car load-path)");
        eprintln!("(car load-path) => {lp}");
        let r = s.eval_to_string("(require 'emacs-buffer-builtins)");
        eprintln!("(require 'emacs-buffer-builtins) => {r}");
        // Soft assertion — just don't panic; the eprintln output is the
        // diagnostic we actually care about.  If the require succeeds
        // we're good; if it fails the eprintln above shows the error.
        if r.starts_with("ERR ") {
            // Try the older path-existing fallback too.
            let r2 = s.eval_to_string("(require 'emacs-error)");
            eprintln!("(require 'emacs-error) fallback => {r2}");
        }
    }
}
