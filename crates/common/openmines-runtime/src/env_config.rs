use anyhow::Result;

pub fn parse_bool_env_or(
    name: &'static str,
    env_val: Option<String>,
    default: bool,
) -> Result<bool> {
    let Some(raw) = env_val else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        value => anyhow::bail!(
            "invalid {name} value {value:?}; expected one of: 1,true,yes,on,0,false,no,off"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_bool_env_or;

    #[test]
    fn accepts_explicit_true_false_and_default() {
        assert!(parse_bool_env_or("X", None, true).unwrap());
        assert!(!parse_bool_env_or("X", None, false).unwrap());
        assert!(parse_bool_env_or("X", Some(" yes ".to_string()), false).unwrap());
        assert!(!parse_bool_env_or("X", Some("OFF".to_string()), true).unwrap());
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(parse_bool_env_or("X", Some(String::new()), false).is_err());
        assert!(parse_bool_env_or("X", Some("wat".to_string()), false).is_err());
        assert!(parse_bool_env_or("X", Some("2".to_string()), false).is_err());
    }
}
