pub fn head_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map(|(i, _)| &s[..i]).unwrap_or(s)
}

pub fn tail_chars(s: &str, n: usize) -> &str {
    let skip = s.chars().count().saturating_sub(n);
    s.char_indices().nth(skip).map(|(i, _)| &s[i..]).unwrap_or("")
}

pub fn preview(s: &str, n: usize) -> String {
    let head = head_chars(s, n);
    if head.len() == s.len() {
        return s.to_owned();
    }
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_chars_does_not_split_multibyte_chars() {
        let s = format!("{}─tail", "a".repeat(499));
        assert_eq!(head_chars(&s, 500), format!("{}─", "a".repeat(499)));
    }

    #[test]
    fn head_chars_returns_whole_string_when_short() {
        assert_eq!(head_chars("héllo", 10), "héllo");
    }

    #[test]
    fn tail_chars_keeps_last_n_chars() {
        assert_eq!(tail_chars("/Users/shané/code", 4), "code");
        assert_eq!(tail_chars("ab", 5), "ab");
        assert_eq!(tail_chars("─a─", 2), "a─");
    }

    #[test]
    fn preview_adds_ellipsis_only_when_truncated() {
        assert_eq!(preview("short", 10), "short");
        assert_eq!(preview("──────", 3), "───…");
    }
}
