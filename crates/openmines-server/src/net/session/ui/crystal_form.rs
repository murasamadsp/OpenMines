pub(super) fn parse_amounts(data: &str) -> Option<[i64; 6]> {
    let mut amounts = [0_i64; 6];
    let mut parts = data.split(':');
    for amount in &mut amounts {
        let part = parts.next()?;
        if part.is_empty() {
            return None;
        }
        *amount = part.parse().ok()?;
    }
    parts.next().is_none().then_some(amounts)
}

#[cfg(test)]
mod tests {
    use super::parse_amounts;

    #[test]
    fn accepts_exactly_six_values() {
        assert_eq!(parse_amounts("1:2:3:4:5:6"), Some([1, 2, 3, 4, 5, 6]));
        assert_eq!(
            parse_amounts("-1:0:10:20:30:40"),
            Some([-1, 0, 10, 20, 30, 40])
        );
    }

    #[test]
    fn rejects_partial_or_malformed_values() {
        assert_eq!(parse_amounts("1:2:3:4:5"), None);
        assert_eq!(parse_amounts("1:2:3:4:5:6:7"), None);
        assert_eq!(parse_amounts("1:x:2:3:4:5:6"), None);
        assert_eq!(parse_amounts("1::2:3:4:5"), None);
    }
}
