// Phase 1.C.1 — minimum viable bridge into the embedded NeLisp runtime.
//
// At this stage we only need to prove that the embedded interpreter is
// alive and reachable from the GTK process.  Future phases will grow
// this module into the full `nelisp-display-*' bridge that elisp Layer
// 2 calls into for frame creation, key event injection, and per-glyph
// face lookup.

use nelisp::eval::eval_str;

/// Evaluate a single elisp form against a fresh global environment and
/// return the printed result, or a printable error string.
pub fn eval_to_string(form: &str) -> String {
    match eval_str(form) {
        Ok(sexp) => format!("{}", sexp),
        Err(err) => format!("ERR {}", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_arithmetic() {
        assert_eq!(eval_to_string("(+ 1 2)"), "3");
    }

    #[test]
    fn multiplication() {
        assert_eq!(eval_to_string("(* 7 8)"), "56");
    }

    #[test]
    fn string_concat() {
        assert_eq!(
            eval_to_string("(concat \"hello, \" \"world\")"),
            "\"hello, world\""
        );
    }

    #[test]
    fn list_length() {
        assert_eq!(eval_to_string("(length '(a b c d e))"), "5");
    }

    #[test]
    fn nested_call() {
        assert_eq!(eval_to_string("(+ (* 3 4) (* 5 6))"), "42");
    }

    #[test]
    fn car_cdr() {
        assert_eq!(eval_to_string("(car (cdr '(a b c)))"), "b");
    }

    #[test]
    fn error_form_returns_err_prefix() {
        let r = eval_to_string("(this-symbol-does-not-exist)");
        assert!(
            r.starts_with("ERR "),
            "expected ERR-prefixed error, got {r:?}"
        );
    }
}
