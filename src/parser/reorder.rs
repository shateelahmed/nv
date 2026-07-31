//! Formatting-preserving key reordering, shared by the dotenv and YAML editors.
//!
//! Reordering moves only **key units** — a key line, the prose `#` comment
//! lines directly above it, and the blank lines directly below it — while every
//! other line (un-attached comments, commented-out assignments, non-key lines)
//! stays pinned at its exact position. Because the pinned lines do not move and
//! the units keep their own lines, the total line count is unchanged, so the
//! permuted units always fill the non-pinned positions exactly and the result
//! is byte-identical everywhere except the moved lines.
//!
//! The format-specific parts — how a key line is recognized and which comment
//! lines may attach to a key — are supplied by the dotenv and YAML editors.

use std::collections::BTreeMap;

use super::comment_text;

/// A key line plus the lines that travel with it: the attached comment block
/// above and the trailing blank separator below.
#[derive(Debug, Clone)]
pub(super) struct KeyUnit {
    /// The key name, used to rank the unit against the target order.
    pub key: String,
    /// The lines of this unit, top to bottom, without line endings.
    pub lines: Vec<String>,
}

/// Reorder the key units in `lines` so they follow `order`, leaving pinned
/// (orphan) lines untouched.
///
/// `key_of` returns the key name of a key line or `None` for any other line.
/// `comment_attaches` decides whether a full-line comment at this position can
/// attach to a key (e.g. it is at the right indentation). `is_commented_out`
/// decides whether a comment's text is a commented-out assignment, which never
/// attaches and breaks any comment block above it. `order` is the target key
/// order; duplicates use the first occurrence.
pub(super) fn reorder_region<F1, F2, F3>(
    lines: &[String],
    key_of: F1,
    comment_attaches: F2,
    is_commented_out: F3,
    order: &[String],
) -> Vec<String>
where
    F1: Fn(&str) -> Option<String>,
    F2: Fn(&str) -> bool,
    F3: Fn(&str) -> bool,
{
    let (units, orphans) = scan(lines, key_of, comment_attaches, is_commented_out);
    let sequence = target_order(&units, order);
    place(lines.len(), &units, &sequence, &orphans)
}

/// Split `lines` into key units (in file order) and pinned orphan lines
/// (recorded with their original index).
fn scan<F1, F2, F3>(
    lines: &[String],
    key_of: F1,
    comment_attaches: F2,
    is_commented_out: F3,
) -> (Vec<KeyUnit>, Vec<(usize, String)>)
where
    F1: Fn(&str) -> Option<String>,
    F2: Fn(&str) -> bool,
    F3: Fn(&str) -> bool,
{
    // Comment lines that may still attach to the next key, with their indexes
    // so they can become pinned lines if the block is broken.
    let mut pending: Vec<(usize, String)> = Vec::new();
    let mut units: Vec<KeyUnit> = Vec::new();
    let mut orphans: Vec<(usize, String)> = Vec::new();
    // Index of the most recent key unit, which may still receive trailing
    // blank lines.
    let mut last_key: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            // A blank line breaks any pending comment block. With none pending
            // it is the trailing separator of the last key unit; otherwise it
            // (and the pending comments) become pinned lines.
            if pending.is_empty() {
                if let Some(ki) = last_key {
                    units[ki].lines.push(line.clone());
                } else {
                    orphans.push((idx, line.clone()));
                }
            } else {
                orphans.append(&mut pending);
                orphans.push((idx, line.clone()));
                last_key = None;
            }
        } else if trimmed.starts_with('#') {
            if comment_attaches(line) {
                let text = comment_text(line);
                if is_commented_out(&text) {
                    // A commented-out assignment never attaches to a key and
                    // breaks any comment block above it.
                    orphans.append(&mut pending);
                    orphans.push((idx, line.clone()));
                    last_key = None;
                } else {
                    pending.push((idx, line.clone()));
                }
            } else {
                // A comment at an indentation that cannot attach (e.g. block
                // documentation inside a k8s block) is pinned in place.
                orphans.append(&mut pending);
                orphans.push((idx, line.clone()));
                last_key = None;
            }
        } else if let Some(key) = key_of(line) {
            let mut unit_lines: Vec<String> = pending.drain(..).map(|(_, l)| l).collect();
            unit_lines.push(line.clone());
            units.push(KeyUnit {
                key,
                lines: unit_lines,
            });
            last_key = Some(units.len() - 1);
        } else {
            // A non-key, non-comment line (block header, document marker, …).
            orphans.append(&mut pending);
            orphans.push((idx, line.clone()));
            last_key = None;
        }
    }

    (units, orphans)
}

/// The index sequence of key units in target order: units whose key is in
/// `order` first (ranked by their position in `order`, first occurrence wins),
/// then units whose key is absent (extras) in their current relative order.
fn target_order(units: &[KeyUnit], order: &[String]) -> Vec<usize> {
    let mut rank: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, key) in order.iter().enumerate() {
        rank.entry(key.as_str()).or_insert(i);
    }

    let mut sequence: Vec<usize> = (0..units.len()).collect();
    sequence.sort_by_key(|&i| match rank.get(units[i].key.as_str()) {
        Some(r) => (*r, 0),
        // Extras sort after every base key and keep their file order.
        None => (usize::MAX, i),
    });
    sequence
}

/// Rebuild the region: pinned orphan lines keep their exact index; every other
/// position is filled, top to bottom, with the permuted units' lines.
fn place(
    len: usize,
    units: &[KeyUnit],
    sequence: &[usize],
    orphans: &[(usize, String)],
) -> Vec<String> {
    let pinned: BTreeMap<usize, &str> = orphans.iter().map(|(i, l)| (*i, l.as_str())).collect();
    let unit_lines: Vec<&str> = sequence
        .iter()
        .flat_map(|&i| units[i].lines.iter())
        .map(|s| s.as_str())
        .collect();

    let mut out = Vec::with_capacity(len);
    let mut cursor = 0usize;
    for idx in 0..len {
        if let Some(line) = pinned.get(&idx) {
            out.push((*line).to_string());
        } else {
            out.push(unit_lines[cursor].to_string());
            cursor += 1;
        }
    }
    // The units and the pinned lines partition the region, so every non-pinned
    // position is filled exactly once.
    debug_assert_eq!(cursor, unit_lines.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the scanner with dotenv-like rules (any comment attaches, keys via
    /// `key=value`).
    fn reorder(lines: &[&str], order: &[&str]) -> Vec<String> {
        let lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        let order: Vec<String> = order.iter().map(|s| s.to_string()).collect();
        reorder_region(
            &lines,
            |l| l.split_once('=').map(|(k, _)| k.trim().to_string()),
            |_| true,
            |text| text.contains('='),
            &order,
        )
    }

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reorders_variable_height_units() {
        let out = reorder(&["# doc C", "C=1", "B=2", "A=3"], &["A", "B", "C"]);
        assert_eq!(out, s(&["A=3", "B=2", "# doc C", "C=1"]));
    }

    #[test]
    fn extras_move_to_bottom_keeping_relative_order() {
        let out = reorder(&["X=1", "C=2", "A=3", "B=4", "Y=5"], &["A", "B", "C"]);
        assert_eq!(out, s(&["A=3", "B=4", "C=2", "X=1", "Y=5"]));
    }

    #[test]
    fn blank_separated_header_stays_pinned() {
        let out = reorder(&["# header", "", "C=3", "", "A=1"], &["A", "C"]);
        assert_eq!(out, s(&["# header", "", "A=1", "C=3", ""]));
    }

    #[test]
    fn commented_out_assignment_breaks_block_and_stays() {
        let out = reorder(&["# doc B", "# KEY=old", "B=2", "A=1"], &["A", "B"]);
        assert_eq!(out, s(&["# doc B", "# KEY=old", "A=1", "B=2"]));
    }

    #[test]
    fn already_in_target_order_is_unchanged() {
        let out = reorder(&["A=1", "B=2", "C=3"], &["A", "B", "C"]);
        assert_eq!(out, s(&["A=1", "B=2", "C=3"]));
    }

    #[test]
    fn trailing_blanks_travel_with_the_unit() {
        let out = reorder(&["C=3", "", "B=2", "", "A=1"], &["A", "B", "C"]);
        assert_eq!(out, s(&["A=1", "B=2", "", "C=3", ""]));
    }

    #[test]
    fn empty_order_leaves_units_unchanged() {
        let out = reorder(&["B=2", "A=1"], &[]);
        assert_eq!(out, s(&["B=2", "A=1"]));
    }

    #[test]
    fn duplicate_keys_use_first_occurrence() {
        let out = reorder(&["C=1", "A=2", "B=3", "B=4"], &["A", "B", "C"]);
        assert_eq!(out, s(&["A=2", "B=3", "B=4", "C=1"]));
    }

    #[test]
    fn unit_lines_may_interleave_around_pinned_lines() {
        // A commented-out assignment is pinned between a comment-bearing key
        // unit and a bare one; the unit's lines end up on both sides of it.
        // This is the documented, deterministic edge case.
        let out = reorder(&["# doc B", "B=2", "# KEY=old", "A=1"], &["A", "B"]);
        assert_eq!(out, s(&["A=1", "# doc B", "# KEY=old", "B=2"]));
    }
}
