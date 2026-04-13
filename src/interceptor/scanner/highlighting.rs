use super::MatchResult;

/// A concrete byte range inside the original command for confirmation UI highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightRange {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

pub(super) fn sorted_highlight_ranges(cmd: &str, matches: &[MatchResult]) -> Vec<HighlightRange> {
    let mut ranges = Vec::with_capacity(matches.len());

    for matched in matches {
        if let Some(range) = matched.highlight_range
            && cmd.get(range.start..range.end).is_some()
        {
            ranges.push(range);
            continue;
        }

        let fragment = matched.matched_text.trim();
        if fragment.is_empty() {
            continue;
        }

        if let Some(start) = cmd.find(fragment) {
            ranges.push(HighlightRange {
                start,
                end: start + fragment.len(),
            });
        }
    }

    if ranges.len() <= 1 {
        return ranges;
    }

    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<HighlightRange> = Vec::with_capacity(ranges.len());

    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }

        merged.push(range);
    }

    merged
}

#[cfg(test)]
pub fn sorted_highlight_ranges_for_tests(
    cmd: &str,
    matches: &[MatchResult],
) -> Vec<HighlightRange> {
    sorted_highlight_ranges(cmd, matches)
}
