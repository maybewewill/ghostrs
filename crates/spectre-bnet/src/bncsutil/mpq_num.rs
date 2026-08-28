

pub fn extract_mpq_number(mpq_name: &str) -> i32 {
    let name = mpq_name.strip_suffix(".mpq").unwrap_or(mpq_name);
    let stem = if let Some(dot_idx) = name.rfind('.') {
        &name[..dot_idx]
    } else {
        name
    };

    let num_start = stem
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let digits = &stem[num_start..];

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
