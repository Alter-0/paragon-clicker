use std::collections::{HashMap, HashSet};
use regex::Regex;

use crate::d2core::model::Step;
use super::score::node_score;
use super::types::{stat_name_en, GlyphDetail, GlyphModel, GlyphRequirement, OptimizationResult, PlannerOptions, RequirementDetail, get_profile_label};

pub fn glyph_radius(rank: u32) -> usize {
    if rank >= 50 {
        5
    } else if rank >= 15 {
        4
    } else {
        3
    }
}

pub fn attribute_amount(step: &Step, name: &str) -> f64 {
    step.attributes
        .iter()
        .filter(|a| a.name == name)
        .map(|a| a.value)
        .sum()
}

pub fn glyph_models(
    nodes: &HashMap<String, Step>,
    options: &PlannerOptions,
) -> Result<Vec<GlyphModel>, String> {
    let re_scaling = Regex::new(r"每购买辐射范围内\s*\d+\s*点(力量|敏捷|智力|意力)").unwrap();
    let mut result = Vec::new();

    for (ref_key, socket) in nodes {
        let Some(ref glyph) = socket.glyph else {
            continue;
        };
        if socket.nodeKind != "socket" {
            continue;
        }

        let rank = options
            .glyph_ranks
            .get(ref_key)
            .copied()
            .or(options.glyph_rank)
            .unwrap_or(glyph.rank);

        let definition = &glyph.definition;
        if definition.is_null() || !definition.is_object() {
            let name = glyph.name.as_deref().unwrap_or(&glyph.key);
            return Err(format!("雕文 {name} 缺少定义，请重新解析 BD"));
        }

        let radius = glyph_radius(rank);
        let center_r = socket.rotatedCoord.row as i32;
        let center_c = socket.rotatedCoord.col as i32;

        let mut nearby: HashMap<String, &Step> = HashMap::new();
        for (other_ref, step) in nodes {
            let r = step.rotatedCoord.row as i32;
            let c = step.rotatedCoord.col as i32;
            let dist = (r - center_r).abs() + (c - center_c).abs();
            if step.boardKey == socket.boardKey && dist > 0 && (dist as usize) <= radius {
                nearby.insert(other_ref.clone(), step);
            }
        }

        let mut requirements = Vec::new();
        if let Some(req_array) = definition.get("threshold_requirements").and_then(|t| t.as_array()) {
            for req_val in req_array {
                let name = req_val.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                let value = req_val.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if value <= 0.0 {
                    continue;
                }

                let mut multiplier = 1.0;
                let bonus_val = definition.get("bonusValue").and_then(|v| v.as_str());
                let bonus_type = definition.get("bonusType");
                if bonus_val == Some("Rare") && bonus_type.is_some() {
                    let bonus = if definition.get("base").is_some() || definition.get("perLevel").is_some() {
                        let base = definition.get("base").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let per_level = definition.get("perLevel").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        base + per_level * (rank.saturating_sub(1) as f64)
                    } else {
                        let max_v = definition.get("max").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let ranks = definition.get("ranks").and_then(|v| v.as_u64()).unwrap_or(100) as f64;
                        let denom = (ranks - 1.0).max(1.0);
                        (max_v / denom) * (rank.saturating_sub(1) as f64)
                    };
                    multiplier += (bonus * 100.0).round() / 100.0 / 100.0;
                }

                let mut amounts = HashMap::new();
                for (other_ref, step) in &nearby {
                    let step_mult = if step.attributes.len() > 1 { multiplier } else { 1.0 };
                    let amt = (attribute_amount(step, name) * 1000.0 * step_mult).round() as i64;
                    amounts.insert(other_ref.clone(), amt);
                }

                requirements.push(GlyphRequirement {
                    name: name.to_string(),
                    required: (value * 1000.0).ceil() as i64,
                    amounts,
                });
            }
        }

        let desc = definition.get("desc").and_then(|d| d.as_str()).unwrap_or_default();
        let mut scaling = HashMap::new();
        if let Some(captures) = re_scaling.captures(desc) {
            if let Some(matched_stat) = captures.get(1) {
                if let Some(en_stat) = stat_name_en(matched_stat.as_str()) {
                    for (other_ref, step) in &nearby {
                        let val = (attribute_amount(step, en_stat) * 30.0).round() as i64;
                        scaling.insert(other_ref.clone(), val);
                    }
                }
            }
        }

        let glyph_name = glyph.name.clone().unwrap_or_else(|| glyph.key.clone());
        result.push(GlyphModel {
            ref_key: ref_key.clone(),
            name: glyph_name,
            rank,
            radius,
            requirements,
            scaling,
        });
    }

    Ok(result)
}

pub fn describe_selection(
    nodes: &HashMap<String, Step>,
    selected: &HashSet<String>,
    char: &str,
    glyphs: &[GlyphModel],
    options: &PlannerOptions,
) -> (i64, usize, usize, f64, Vec<GlyphDetail>) {
    let mut score = selected
        .iter()
        .filter_map(|r| nodes.get(r))
        .map(|s| node_score(s, char, &options.profile))
        .sum::<i64>();

    let mut glyph_details = Vec::new();
    for glyph in glyphs {
        let socket_selected = selected.contains(&glyph.ref_key) && glyph.rank > 0;
        let mut req_details = Vec::new();

        let mut active_reqs = true;
        if glyph.requirements.is_empty() {
            active_reqs = false;
        }

        for req in &glyph.requirements {
            let mut actual_sum: i64 = 0;
            for (r, amt) in &req.amounts {
                if selected.contains(r) {
                    actual_sum += amt;
                }
            }
            if actual_sum < req.required {
                active_reqs = false;
            }
            req_details.push(RequirementDetail {
                name: req.name.clone(),
                required: req.required as f64 / 1000.0,
                actual: actual_sum as f64 / 1000.0,
            });
        }

        let active = socket_selected && active_reqs;
        if socket_selected {
            score += 500;
            for (r, val) in &glyph.scaling {
                if selected.contains(r) {
                    score += val;
                }
            }
        }
        if active {
            score += 10000;
        }

        glyph_details.push(GlyphDetail {
            name: glyph.name.clone(),
            rank: glyph.rank,
            radius: glyph.radius,
            socketSelected: socket_selected,
            active,
            requirements: req_details,
        });
    }

    let legendary_count = selected
        .iter()
        .filter_map(|r| nodes.get(r))
        .filter(|s| s.nodeKind == "legendary")
        .count();

    let active_glyph_count = glyph_details.iter().filter(|g| g.active).count();

    let mut life_percent = 0.0;
    for r in selected {
        if let Some(step) = nodes.get(r) {
            for a in &step.attributes {
                if a.name.contains("HPMaxBonus") {
                    life_percent += a.value;
                }
            }
        }
    }

    (score, legendary_count, active_glyph_count, life_percent, glyph_details)
}

pub fn make_optimization_result(
    nodes: &HashMap<String, Step>,
    selected: &HashSet<String>,
    char: &str,
    glyphs: &[GlyphModel],
    options: &PlannerOptions,
    status: &str,
    optimal: bool,
    objective: f64,
    upper_bound: f64,
    seconds: f64,
    available_points: usize,
    free_count: usize,
) -> OptimizationResult {
    let (score, legendary_count, active_glyph_count, life_percent, glyph_details) =
        describe_selection(nodes, selected, char, glyphs, options);

    let spent = selected.len().saturating_sub(free_count);
    let unspent = available_points.saturating_sub(spent);

    OptimizationResult {
        score,
        legendaryCount: legendary_count,
        activeGlyphCount: active_glyph_count,
        lifePercent: life_percent,
        glyphs: glyph_details,
        profile: options.profile.clone(),
        profileLabel: get_profile_label(&options.profile).to_string(),
        status: status.to_string(),
        optimal,
        objective,
        upperBound: upper_bound,
        seconds,
        budget: available_points,
        unspentPoints: unspent,
        assumptions: vec![
            "评分是明确的加点优先级，不是伤害模拟；传奇节点按等权处理。".to_string(),
            "保留 BD 的板序、旋转和目标节点；未搜索 BD 之外的捷径。".to_string(),
            "未计入装备、战斗覆盖率、稀有节点额外奖励及未建模的雕文放大效果。".to_string(),
            "雕文范围按暗黑核规则；请设置每枚雕文的实际等级，0 表示未装备。".to_string(),
        ],
    }
}
