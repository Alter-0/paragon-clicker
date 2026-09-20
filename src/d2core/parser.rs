use regex::Regex;
use url::Url;

pub fn parse_planner_input(value: &str) -> Result<(String, usize), String> {
    let text = value.trim();
    if text.is_empty() {
        return Err("Planner URL is empty".to_string());
    }

    let id_re = Regex::new(r"^[A-Za-z0-9_-]+$").unwrap();
    if id_re.is_match(text) {
        return Ok((text.to_string(), 0));
    }

    let parsed = Url::parse(text).map_err(|e| format!("Invalid planner URL: {e}"))?;
    if parsed.scheme().is_empty() || parsed.host_str().is_none() {
        return Err("Invalid planner URL".to_string());
    }

    let mut bd_val: Option<String> = None;
    let mut var_val: usize = 0;

    for (k, v) in parsed.query_pairs() {
        if k == "bd" {
            let s = v.trim();
            if !s.is_empty() {
                bd_val = Some(s.to_string());
            }
        } else if k == "var" {
            let s = v.trim();
            if !s.is_empty() {
                var_val = s.parse::<usize>().map_err(|_| "变体编号 var 必须是整数".to_string())?;
            }
        }
    }

    let bd = bd_val.ok_or_else(|| "Planner URL does not contain bd=".to_string())?;
    Ok((bd, var_val))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_input() {
        assert_eq!(parse_planner_input("1Tok").unwrap(), ("1Tok".to_string(), 0));
        assert_eq!(
            parse_planner_input("https://www.d2core.com/d4/planner?bd=1Tok").unwrap(),
            ("1Tok".to_string(), 0)
        );
        assert_eq!(
            parse_planner_input("https://www.d2core.com/d4/planner?bd=23tb&var=3").unwrap(),
            ("23tb".to_string(), 3)
        );
        assert!(parse_planner_input("").is_err());
        assert!(parse_planner_input("https://www.d2core.com/d4/planner?other=1").is_err());
    }
}
