pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == 'm' || next == 'K' || next == 'H' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == '\x07' || next == '\\' {
                        break;
                    }
                }
            }
        } else if c.is_ascii_control() && c != '\n' && c != '\t' {
            continue;
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_basic_color() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_multiple_codes() {
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m text"), "bold green text");
    }

    #[test]
    fn test_preserves_newlines() {
        assert_eq!(strip_ansi("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn test_no_ansi() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }
}
