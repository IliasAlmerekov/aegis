/// Derive program keys for the `by_program` index from a regex pattern string.
///
/// Only patterns that are anchored to the start of the string (`^`) are indexed by
/// program. Non-anchored patterns (e.g. `\brm\s+`) can match anywhere in a command
/// string — including inside quoted arguments — and must run for every command; they
/// belong in the `universal` set.
///
/// When the anchor is followed by a name with an optional simple char class (e.g.
/// `^python[23]?\s+`), ALL concrete variants are returned (`"python"`, `"python2"`,
/// `"python3"`). Ranges (e.g. `[a-z]`) are not expanded to avoid combinatorial blow-up.
///
/// Returns an empty `Vec` for non-anchored patterns (universal) and for patterns
/// whose anchor is not followed by a recognisable program name.
pub(super) fn derive_program_keys(pattern: &str) -> Vec<String> {
    let s = pattern.strip_prefix("(?i)").unwrap_or(pattern);
    // Accept only if every top-level alternative starts with `^`.
    if !split_top_alternation(s)
        .iter()
        .all(|alt| alt.starts_with('^'))
    {
        return Vec::new();
    }
    // Derive keys per alternative, expanding simple optional char classes.
    let mut all_keys: Vec<String> = Vec::new();
    for alt in split_top_alternation(s) {
        let body = alt.strip_prefix('^').unwrap_or(alt);
        for key in expanded_leading_keys(body) {
            let key_lower = key.to_ascii_lowercase();
            if is_program_name(&key_lower) && !all_keys.contains(&key_lower) {
                all_keys.push(key_lower);
            }
        }
    }
    all_keys
}

/// Extract the leading literal(s) from a `^`-stripped pattern alternative.
///
/// When the literal is followed by an optional char class without ranges (e.g.
/// `[23]?`), returns the base literal plus one variant per class character.
/// This lets `^python[23]?\s+` index under `"python"`, `"python2"`, `"python3"`.
fn expanded_leading_keys(s: &str) -> Vec<String> {
    let base = leading_literal(s);
    if base.is_empty() {
        return Vec::new();
    }
    let after_base = &s[base.len()..];
    // Detect `[chars]?` immediately after the literal.
    if let Some(rest) = after_base.strip_prefix('[')
        && let Some(bracket_end) = rest.find(']')
    {
        let class_content = &rest[..bracket_end];
        let after_bracket = &rest[bracket_end + 1..];
        // Only expand optional (`?`) classes with no ranges (`-`).
        if after_bracket.starts_with('?') && !class_content.contains('-') {
            let mut keys = vec![base.clone()];
            for c in class_content.chars().filter(|c| c.is_ascii_alphanumeric()) {
                keys.push(format!("{base}{c}"));
            }
            return keys;
        }
    }
    vec![base]
}

fn is_program_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        _ => false,
    }
}

pub(super) fn extract_keywords(pattern: &str) -> Vec<String> {
    let s = pattern.strip_prefix("(?i)").unwrap_or(pattern);
    extract_inner(s)
}

fn extract_inner(s: &str) -> Vec<String> {
    // When an optional literal prefix exists (e.g. `(ba)?sh`), also emit the
    // combined keyword (`bash`) so the by-program index covers both spellings.
    let combined = combined_optional_prefix_keyword(s);

    let s = strip_leading_optional_group(s);

    let parts = split_top_alternation(s);
    let mut keywords = if parts.len() > 1 {
        parts.into_iter().flat_map(extract_inner).collect()
    } else {
        let lit = leading_literal(s);
        if lit.len() >= 2 {
            vec![lit.to_ascii_lowercase()]
        } else {
            find_embedded_literal(s)
                .map(|l| vec![l.to_ascii_lowercase()])
                .unwrap_or_default()
        }
    };

    if let Some(c) = combined {
        let c_lower = c.to_ascii_lowercase();
        if !keywords.contains(&c_lower) {
            keywords.push(c_lower);
        }
    }

    keywords
}

/// If `s` starts with `(literal)?rest`, returns `literal + leading_literal(rest)`.
///
/// Only fires when the optional group is a plain literal (no whitespace metacharacters
/// inside), which means it is a prefix fragment of the program name rather than a
/// separate optional token (e.g. `(ba)?sh` → "bash", but `(sudo\s+)?rm` → None).
fn combined_optional_prefix_keyword(s: &str) -> Option<String> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut close_idx = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_idx = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if close_idx == 0 {
        return None;
    }
    // Must be an optional group: `)` followed immediately by `?`
    let after_group = &s[close_idx + 1..];
    if !after_group.starts_with('?') {
        return None;
    }
    // Prefix content must be a plain literal (no whitespace patterns or metacharacters).
    let group_content = &s[1..close_idx];
    let prefix_lit = leading_literal(group_content);
    if prefix_lit != group_content {
        return None;
    }
    // Combine prefix with the leading literal of the remaining pattern.
    let remaining = &s[close_idx + 2..];
    let suffix_lit = leading_literal(remaining);
    if suffix_lit.len() >= 2 {
        Some(prefix_lit + &suffix_lit)
    } else {
        None
    }
}

fn strip_leading_optional_group(s: &str) -> &str {
    if !s.starts_with('(') {
        return s;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let after = &s[i + 1..];
                    if let Some(stripped) = after.strip_prefix('?') {
                        return stripped;
                    }
                    return s;
                }
            }
            _ => {}
        }
    }
    s
}

fn split_top_alternation(s: &str) -> Vec<&str> {
    let mut depth: i32 = 0;
    let mut last = 0usize;
    let mut parts: Vec<&str> = Vec::new();
    let mut chars = s.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[last..]);
    parts
}

fn leading_literal(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('b' | 'B') => {
                    chars.next();
                }
                Some('s' | 'S' | 'd' | 'D' | 'w' | 'W' | 'n' | 'r' | 't' | 'f' | 'v' | 'a') => {
                    break;
                }
                Some(_) => {
                    if let Some(next_c) = chars.next() {
                        result.push(next_c);
                    }
                }
                None => break,
            },
            '.' | '+' | '*' | '?' | '[' | '{' | '(' | ')' | '^' | '$' | '|' => break,
            _ => result.push(c),
        }
    }

    result.trim_end().to_string()
}

fn find_embedded_literal(s: &str) -> Option<String> {
    let mut current = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('b' | 'B') => {
                    chars.next();
                }
                Some('s' | 'S' | 'd' | 'D' | 'w' | 'W' | 'n' | 'r' | 't' | 'f' | 'v' | 'a') => {
                    chars.next();
                    if current.trim_end().len() >= 3 {
                        return Some(current.trim_end().to_string());
                    }
                    current.clear();
                }
                Some(_) => {
                    if let Some(next_c) = chars.next() {
                        current.push(next_c);
                    }
                }
                None => break,
            },
            '.' | '+' | '*' | '?' | '{' | '}' | '(' | ')' | '^' | '$' | '|' => {
                if current.trim_end().len() >= 3 {
                    return Some(current.trim_end().to_string());
                }
                current.clear();
            }
            '[' => {
                if current.trim_end().len() >= 3 {
                    return Some(current.trim_end().to_string());
                }
                current.clear();
                for c2 in chars.by_ref() {
                    if c2 == ']' {
                        break;
                    }
                }
            }
            _ => current.push(c),
        }
    }

    if current.trim_end().len() >= 3 {
        Some(current.trim_end().to_string())
    } else {
        None
    }
}

#[cfg(test)]
pub(super) fn leading_literal_for_tests(s: &str) -> String {
    leading_literal(s)
}

#[cfg(test)]
pub(super) fn split_top_alternation_for_tests(s: &str) -> Vec<&str> {
    split_top_alternation(s)
}

#[cfg(test)]
pub(super) fn strip_leading_optional_group_for_tests(s: &str) -> &str {
    strip_leading_optional_group(s)
}

#[cfg(test)]
mod tests {
    fn production_source_for_tests(source: &str) -> &str {
        let boundary = source
            .find("\n#[cfg(test)]\nmod tests {")
            .expect("test module boundary must exist");
        &source[..boundary]
    }

    #[test]
    fn production_source_for_tests_skips_test_module_literal_needles() {
        let synthetic_source = "\
fn hot_path() {}
#[cfg(test)]
pub(super) fn helper() {}

#[cfg(test)]
mod tests {
    #[test]
    fn guard() {
        assert!(!\"chars.next().unwrap()\".is_empty());
    }
}
";

        let production_source = production_source_for_tests(synthetic_source);
        assert!(
            !production_source.contains("chars.next().unwrap()"),
            "production slice must stop before the test module even when earlier #[cfg(test)] helpers exist"
        );
    }

    #[test]
    fn keyword_extraction_hot_path_avoids_next_unwrap() {
        let raw = include_str!("keywords.rs");
        let source = raw.replace("\r\n", "\n");
        let production_source = production_source_for_tests(&source);
        assert!(
            !production_source.contains("chars.next().unwrap()"),
            "keywords extraction hot path must not use chars.next().unwrap() in production code"
        );
    }
}
