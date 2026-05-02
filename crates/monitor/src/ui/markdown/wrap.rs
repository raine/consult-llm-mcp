pub(super) fn wrap_code_lines(code: &str, max_width: usize) -> String {
    if max_width == 0 {
        return code.to_string();
    }

    let mut result = String::new();
    for line in code.lines() {
        let line_width = line.chars().count();
        if line_width <= max_width {
            result.push_str(line);
            result.push('\n');
        } else {
            let mut current_width = 0;
            for ch in line.chars() {
                let ch_width = 1; // simplified from UnicodeWidthChar
                if current_width + ch_width > max_width && current_width > 0 {
                    result.push('\n');
                    current_width = 0;
                }
                result.push(ch);
                current_width += ch_width;
            }
            result.push('\n');
        }
    }
    result
}
