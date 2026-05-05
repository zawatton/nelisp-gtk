// Bridge module — owns the persistent NeLisp `Env' and exposes:
//
//   - `Session::new()`        : empty global env
//   - `Session::env_mut()`    : raw mutable handle for builtin registration
//   - `Session::eval_form(...)`: read-eval, surfacing errors as `BridgeError'
//   - `Session::eval_to_string(...)`: convenience for tests + diagnostics
//   - `layer2_setup_form()`   : the boot elisp that loads the substrate
//
// All elisp policy (= mode-line composition, key dispatch, draw refresh)
// lives in `nemacs-gtk-frontend.el' on the substrate side; this file
// only carries the boot form so the binary can `(require ...)` the
// frontend.

use nelisp::eval::{eval, Env, EvalError};
use nelisp::reader::{read_str, ReadError, Sexp};

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

pub struct Session {
    env: Env,
}

impl Session {
    pub fn new() -> Self {
        Self { env: Env::new_global() }
    }

    /// Mutable access to the underlying `Env' — used at boot time for
    /// `register_extern_builtin' calls.  Caller must not retain the
    /// borrow across `eval_form' / `eval_to_string' invocations.
    pub fn env_mut(&mut self) -> &mut Env {
        &mut self.env
    }

    pub fn eval_form(&mut self, form_str: &str) -> Result<Sexp, BridgeError> {
        let form = read_str(form_str).map_err(BridgeError::Read)?;
        eval(&form, &mut self.env).map_err(BridgeError::Eval)
    }

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
///      by the active substrate worktree).  Both the direct
///      (`Cowork/Notes/...') and symlinked (`Notes/...') prefixes are
///      tried.
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
        format!(
            "{home}/Cowork/Notes/dev/nelisp-emacs/.worktrees/emacs-builtins-port/src"
        ),
        format!(
            "{home}/Notes/dev/nelisp-emacs/.worktrees/emacs-builtins-port/src"
        ),
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
    candidates[3].clone()
}

pub fn layer2_home_path() -> String {
    let src = layer2_src_path();
    if let Some(stripped) = src.strip_suffix("/src") {
        stripped.to_string()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{}/Notes/dev/nelisp-emacs", home)
    }
}

/// Boot form that primes a fresh Session with the Layer 2 load-path,
/// `nelisp-emacs-vendor-root', and `(require 'emacs-init)' which is
/// the canonical master require chain pulling in every sibling
/// substrate module.  Idempotent — safe to run multiple times against
/// the same Session.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip() {
        let mut s = Session::new();
        assert_eq!(s.eval_to_string("(+ 1 2)"), "3");
    }

    #[test]
    fn layer2_setup_form_loads_substrate() {
        let mut s = Session::new();
        let r = s.eval_to_string(&layer2_setup_form());
        assert!(!r.starts_with("ERR "), "layer2 setup failed: {r}");
        assert_eq!(s.eval_to_string("(fboundp 'window-system)"), "t");
    }

    #[test]
    fn env_mut_allows_extern_builtin_registration() {
        let mut s = Session::new();
        s.env_mut()
            .register_extern_builtin("test-extern-double", |args, _| match args.get(0) {
                Some(Sexp::Int(n)) => Ok(Sexp::Int(n * 2)),
                _ => Err(EvalError::ArithError("expected int".into())),
            });
        assert_eq!(s.eval_to_string("(test-extern-double 21)"), "42");
    }
}
