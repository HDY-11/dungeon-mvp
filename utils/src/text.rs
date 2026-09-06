//! 纯文本工具。

/// 按字符数安全截断 UTF-8 字符串。
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut end = 0;
    for (idx, (byte_idx, _)) in s.char_indices().enumerate() {
        if idx == max_chars {
            break;
        }
        end = byte_idx + s[byte_idx..].chars().next().map(char::len_utf8).unwrap_or(0);
    }
    &s[..end]
}

/// 按字符数截断并返回拥有所有权的字符串。
pub fn truncate_chars_owned(s: &str, max_chars: usize) -> String {
    truncate_chars(s, max_chars).to_owned()
}

/// 计算一个可见窗口的起始索引，使 `sel` 尽量居中且不越界。
pub fn window_start(sel: usize, len: usize, window_len: usize) -> usize {
    if len <= window_len || sel < window_len / 2 {
        0
    } else {
        (sel - window_len / 2).min(len - window_len)
    }
}
