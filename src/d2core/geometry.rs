use std::collections::HashMap;
use regex::Regex;
use serde_json::Value;

use super::model::{Board, Coord, SelectedNode, ThresholdRequirement};

pub const GRID_SIZE: usize = 21;
pub const NODE_NUM: usize = 21;
pub const DELTAS: [(i32, i32); 4] = [(-1, 0), (0, -1), (0, 1), (1, 0)];

pub fn parse_node_key(node_key: &str) -> (usize, usize, String) {
    let parts: Vec<&str> = node_key.split('_').collect();
    let row = parts[0].parse::<usize>().unwrap_or(0);
    let col = parts[1].parse::<usize>().unwrap_or(0);
    let node_id = parts[2..].join("_");
    (row, col, node_id)
}

pub fn get_rotated_pos(row: usize, col: usize, rotate: i32) -> Coord {
    let mut next_row = row;
    let mut next_col = col;
    let r = ((rotate % 4) + 4) % 4;
    for _ in 0..r {
        let prev_row = next_row;
        next_row = next_col;
        next_col = NODE_NUM - 1 - prev_row;
    }
    Coord { row: next_row, col: next_col }
}

pub fn get_connect_board_pos(x: i32, y: i32, row: usize, col: usize) -> (i32, i32, usize, usize) {
    let mut dx = 0;
    let mut dy = 0;
    let mut next_row = 0;
    let mut next_col = 0;

    if row == 0 {
        dy = -1;
        next_row = NODE_NUM - 1;
        next_col = col;
    } else if row == NODE_NUM - 1 {
        dy = 1;
        next_row = 0;
        next_col = col;
    } else if col == 0 {
        dx = -1;
        next_row = row;
        next_col = NODE_NUM - 1;
    } else if col == NODE_NUM - 1 {
        dx = 1;
        next_row = row;
        next_col = 0;
    }

    (x + dx, y + dy, next_row, next_col)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeConstraint {
    Row(usize),
    Col(usize),
}

pub fn get_parent_entry_edge(
    board: &Board,
    boards_by_key: &HashMap<String, &Board>,
) -> Option<EdgeConstraint> {
    let parent_key = board.parent.as_ref()?;
    let parent = boards_by_key.get(parent_key)?;
    let dx = board.position.x - parent.position.x;
    let dy = board.position.y - parent.position.y;

    if dy == -1 {
        Some(EdgeConstraint::Row(NODE_NUM - 1))
    } else if dy == 1 {
        Some(EdgeConstraint::Row(0))
    } else if dx == -1 {
        Some(EdgeConstraint::Col(NODE_NUM - 1))
    } else if dx == 1 {
        Some(EdgeConstraint::Col(0))
    } else {
        None
    }
}

pub fn is_cell_on_edge(cell: &SelectedNode, edge: Option<EdgeConstraint>) -> bool {
    match edge {
        Some(EdgeConstraint::Row(r)) => cell.rotated.row == r,
        Some(EdgeConstraint::Col(c)) => cell.rotated.col == c,
        None => false,
    }
}

pub fn get_node_kind(node_id: &str) -> String {
    if node_id.contains("StartNode") {
        return "start".to_string();
    }
    if node_id == "Generic_Gate" {
        return "gate".to_string();
    }
    if node_id == "Generic_Socket" {
        return "socket".to_string();
    }
    let parts: Vec<&str> = node_id.split('_').collect();
    if parts.len() > 1 {
        parts[1].to_lowercase()
    } else {
        "normal".to_string()
    }
}

pub fn fallback_node_name(node_id: &str) -> String {
    node_id.replace('_', " ")
}

pub fn eval_safe_expr(expr: &str) -> Option<f64> {
    // Simple recursive descent parser for basic math: +, -, *, /, ()
    let chars: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut pos = 0;
    parse_additive(&chars, &mut pos)
}

fn parse_additive(chars: &[char], pos: &mut usize) -> Option<f64> {
    let mut val = parse_multiplicative(chars, pos)?;
    while *pos < chars.len() {
        match chars[*pos] {
            '+' => {
                *pos += 1;
                let next_val = parse_multiplicative(chars, pos)?;
                val += next_val;
            }
            '-' => {
                *pos += 1;
                let next_val = parse_multiplicative(chars, pos)?;
                val -= next_val;
            }
            _ => break,
        }
    }
    Some(val)
}

fn parse_multiplicative(chars: &[char], pos: &mut usize) -> Option<f64> {
    let mut val = parse_primary(chars, pos)?;
    while *pos < chars.len() {
        match chars[*pos] {
            '*' => {
                *pos += 1;
                let next_val = parse_primary(chars, pos)?;
                val *= next_val;
            }
            '/' => {
                *pos += 1;
                let next_val = parse_primary(chars, pos)?;
                if next_val == 0.0 { return None; }
                val /= next_val;
            }
            _ => break,
        }
    }
    Some(val)
}

fn parse_primary(chars: &[char], pos: &mut usize) -> Option<f64> {
    if *pos >= chars.len() {
        return None;
    }
    if chars[*pos] == '(' {
        *pos += 1;
        let val = parse_additive(chars, pos)?;
        if *pos < chars.len() && chars[*pos] == ')' {
            *pos += 1;
            return Some(val);
        }
        return None;
    }
    if chars[*pos] == '-' {
        *pos += 1;
        let val = parse_primary(chars, pos)?;
        return Some(-val);
    }
    let start = *pos;
    while *pos < chars.len() && (chars[*pos].is_ascii_digit() || chars[*pos] == '.') {
        *pos += 1;
    }
    if start == *pos {
        return None;
    }
    let s: String = chars[start..*pos].iter().collect();
    s.parse::<f64>().ok()
}

pub fn resolve_thresholds(
    node_def: Option<&Value>,
    char: &str,
    board_index: i32,
) -> Vec<ThresholdRequirement> {
    let Some(node_def) = node_def else {
        return Vec::new();
    };
    let Some(requirements) = node_def
        .get("threshold_requirements")
        .and_then(|t| t.get(char))
        .and_then(|c| c.as_array())
    else {
        return Vec::new();
    };

    let safe_re = Regex::new(r"^[0-9+*\- /().A-Za-z]+$").unwrap();
    let num_re = Regex::new(r"^[0-9.+\-]+$").unwrap();
    let mut resolved_list = Vec::new();

    for requirement in requirements {
        let raw = requirement.get("value").and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(n) = v.as_f64() {
                Some(n.to_string())
            } else if let Some(i) = v.as_i64() {
                Some(i.to_string())
            } else {
                None
            }
        }).unwrap_or_default();

        let mut resolved: Option<f64> = None;
        if safe_re.is_match(&raw) && raw.contains("ParagonBoardEquipIndex") {
            let expr = raw.replace("ParagonBoardEquipIndex", &board_index.to_string());
            resolved = eval_safe_expr(&expr);
        } else if num_re.is_match(&raw) {
            resolved = raw.parse::<f64>().ok();
        }

        resolved_list.push(ThresholdRequirement {
            name: requirement.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
            raw,
            resolved,
        });
    }
    resolved_list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation() {
        assert_eq!(get_rotated_pos(0, 0, 0), Coord { row: 0, col: 0 });
        assert_eq!(get_rotated_pos(0, 0, 1), Coord { row: 0, col: 20 });
        assert_eq!(get_rotated_pos(0, 0, 2), Coord { row: 20, col: 20 });
        assert_eq!(get_rotated_pos(0, 0, 3), Coord { row: 20, col: 0 });
    }

    #[test]
    fn test_eval_safe_expr() {
        assert_eq!(eval_safe_expr("160+20*(3-1)"), Some(200.0));
        assert_eq!(eval_safe_expr("250 + 30 * 2"), Some(310.0));
    }
}
