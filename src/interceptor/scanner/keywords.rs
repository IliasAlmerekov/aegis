pub(super) fn extract_keywords(pattern: &str) -> Vec<String> {
    let s = pattern.strip_prefix("(?i)").unwrap_or(pattern);
    extract_inner(s)
}

fn extract_inner(s: &str) -> Vec<String> {
    let s = strip_leading_optional_group(s);

    let parts = split_top_alternation(s);
    if parts.len() > 1 {
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
                Some(
                    's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'b' | 'B' | 'n' | 'r' | 't' | 'f' | 'v'
                    | 'a',
                ) => break,
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
                Some(
                    's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'b' | 'B' | 'n' | 'r' | 't' | 'f' | 'v'
                    | 'a',
                ) => {
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
