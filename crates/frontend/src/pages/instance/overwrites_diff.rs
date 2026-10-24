//! Index projection for a unified diff. The underlying paired lines remain unchanged.
#[derive(Clone, Debug)]
pub(crate) struct DiffRow {
    pub left: Option<(usize, String)>,
    pub right: Option<(usize, String)>,
    pub changed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisplayRow {
    pub pair: usize,
    pub local: bool,
}
pub(crate) type Hunk = (usize, usize, bool, bool);
pub(crate) fn unified_projection(rows: &[DiffRow], hunks: &[Hunk]) -> (Vec<DisplayRow>, Vec<Hunk>) {
    let mut display = Vec::with_capacity(rows.len());
    let mut offsets = Vec::with_capacity(rows.len() + 1);
    for (pair, row) in rows.iter().enumerate() {
        offsets.push(display.len());
        if row.changed {
            // Parent/removals precede local/additions, as in the approved unified view.
            if row.right.is_some() {
                display.push(DisplayRow { pair, local: false });
            }
            if row.left.is_some() {
                display.push(DisplayRow { pair, local: true });
            }
        } else if row.left.is_some() || row.right.is_some() {
            display.push(DisplayRow {
                pair,
                local: row.left.is_some(),
            });
        }
    }
    offsets.push(display.len());
    let mapped = hunks
        .iter()
        .filter_map(|&(start, count, local, parent)| {
            let end = start.checked_add(count)?;
            Some((*offsets.get(start)?, offsets.get(end)?.checked_sub(*offsets.get(start)?)?, local, parent))
        })
        .collect();
    (display, mapped)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn line(n: usize, text: &str) -> Option<(usize, String)> {
        Some((n, text.into()))
    }
    #[test]
    fn replacement_keeps_parent_before_local_and_unchanged_lines_once() {
        let rows = vec![
            DiffRow {
                left: line(1, "[opción]"),
                right: line(1, "[opción]"),
                changed: false,
            },
            DiffRow {
                left: line(2, "activado=false"),
                right: line(2, "activado=true"),
                changed: true,
            },
            DiffRow {
                left: None,
                right: line(3, "quitado=true"),
                changed: true,
            },
            DiffRow {
                left: line(3, "añadido=true"),
                right: None,
                changed: true,
            },
        ];
        let (display, hunks) = unified_projection(&rows, &[(1, 3, true, true)]);
        let texts = display
            .iter()
            .map(|d| {
                let row = &rows[d.pair];
                if d.local { &row.left } else { &row.right }
            })
            .map(|line| line.as_ref().unwrap().1.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                "[opción]",
                "activado=true",
                "activado=false",
                "quitado=true",
                "añadido=true"
            ]
        );
        assert_eq!(hunks, [(1, 4, true, true)]);
    }
    #[test]
    fn later_change_markers_account_for_expanded_replacements() {
        let rows = vec![
            DiffRow {
                left: line(1, "nuevo"),
                right: line(1, "anterior"),
                changed: true,
            },
            DiffRow {
                left: line(2, "igual"),
                right: line(2, "igual"),
                changed: false,
            },
            DiffRow {
                left: line(3, "solo local"),
                right: None,
                changed: true,
            },
        ];
        let (_, hunks) = unified_projection(&rows, &[(0, 1, true, true), (2, 1, true, false)]);
        assert_eq!(hunks, [(0, 2, true, true), (3, 1, true, false)]);
    }
}
