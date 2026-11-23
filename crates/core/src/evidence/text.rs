//! The page as text, compared line by line: what a model (or a person)
//! reads to see what changed on a page without looking at it.
//!
//! ```text
//! - button "Save"
//! + button "Save all"
//! ```
//!
//! A longest-common-subsequence diff over lines: lines only in the
//! baseline are `- `, lines only in the capture are `+ `, in page order.
//! Pure.

/// Most lines compared each side; beyond it the rest counts as changed
/// wholesale (an AX tree is a few hundred lines).
pub const MAX_LINES: usize = 4_000;

/// The changed lines, `- old` and `+ new`, in page order. Empty: the same.
pub fn diff(before: &str, after: &str) -> Vec<String> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    // The same start and end need no table.
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return a.iter().map(|l| format!("- {l}")).chain(b.iter().map(|l| format!("+ {l}"))).collect();
    }
    // lcs[i][j]: the common lines of a[i..] and b[j..].
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] { lcs[at(i + 1, j + 1)] + 1 } else { lcs[at(i + 1, j)].max(lcs[at(i, j + 1)]) };
        }
    }
    // Walk the table; each run of changes between two common lines is a
    // hunk, written removals first, so a changed line reads `- old`, `+ new`.
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    let (mut minus, mut plus): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let flush = |out: &mut Vec<String>, minus: &mut Vec<String>, plus: &mut Vec<String>| {
        out.append(minus);
        out.append(plus);
    };
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            flush(&mut out, &mut minus, &mut plus);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[at(i, j + 1)] >= lcs[at(i + 1, j)]) {
            plus.push(format!("+ {}", b[j]));
            j += 1;
        } else {
            minus.push(format!("- {}", a[i]));
            i += 1;
        }
    }
    flush(&mut out, &mut minus, &mut plus);
    out
}

#[cfg(test)]
mod tests {
    use super::diff;

    #[test]
    fn the_same_text_has_no_lines() {
        assert!(diff("a\nb\nc", "a\nb\nc").is_empty());
        assert!(diff("", "").is_empty());
    }

    #[test]
    fn a_changed_line_is_minus_then_plus() {
        assert_eq!(
            diff("heading \"Home\"\nbutton \"Save\"\ntext \"x\"", "heading \"Home\"\nbutton \"Save all\"\ntext \"x\""),
            ["- button \"Save\"", "+ button \"Save all\""]
        );
    }

    #[test]
    fn added_and_removed_lines_keep_their_order() {
        assert_eq!(diff("a\nb\nc\nd", "a\nc\nd\ne"), ["- b", "+ e"]);
        assert_eq!(diff("", "x\ny"), ["+ x", "+ y"]);
        assert_eq!(diff("a\nb\nc\nd\ne", "a\nB\nc\nd\nE"), ["- b", "+ B", "- e", "+ E"], "each hunk on its own");
        assert_eq!(diff("x\ny", ""), ["- x", "- y"]);
    }
}
