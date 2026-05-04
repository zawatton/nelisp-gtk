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
///   3. `$HOME/Notes/dev/nelisp-emacs/src` (= canonical dev layout)
pub fn layer2_src_path() -> String {
    if let Ok(home) = std::env::var("NEMACS_HOME") {
        return format!("{}/src", home.trim_end_matches('/'));
    }
    if let Ok(p) = std::env::var("NEMACS_LAYER2_SRC") {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    format!("{}/Notes/dev/nelisp-emacs/src", home)
}

/// Boot form that primes a fresh Session with the Layer 2 load-path.
/// Returns the elisp source string ready to feed to
/// [`Session::eval_form`].  Idempotent — safe to run multiple times.
pub fn layer2_setup_form() -> String {
    format!(
        r#"(progn
            (unless (boundp 'load-path) (defvar load-path nil))
            (unless (member "{src}" load-path)
              (setq load-path (cons "{src}" load-path)))
            'load-path-ready)"#,
        src = layer2_src_path()
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
}
