/// Extracts the integer version number from an MPQ filename (e.g. "IX86ver1.mpq" -> 1).
/// If no digit sequence is found, defaults to 1.
pub fn extract_mpq_number(mpq_name: &str) -> i32 {
    let name = mpq_name.strip_suffix(".mpq").unwrap_or(mpq_name);
    let stem = if let Some(dot_idx) = name.rfind('.') {
        &name[..dot_idx]
    } else {
        name
    };

    let digits: String = stem
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    if digits.is_empty() {
        1
    } else {
        digits.parse::<i32>().unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mpq_digits_correctly() {
        assert_eq!(extract_mpq_number("IX86ver1.mpq"), 1);
        assert_eq!(extract_mpq_number("IX86ver2.mpq"), 2);
        assert_eq!(extract_mpq_number("ver10.mpq"), 10);
        assert_eq!(extract_mpq_number("PMACver7.mpq"), 7);
        assert_eq!(extract_mpq_number("ver-IX86-1.mpq"), 1);
        assert_eq!(extract_mpq_number("no_numbers.mpq"), 1);
        assert_eq!(extract_mpq_number(""), 1);
    }
}
