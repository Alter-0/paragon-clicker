#![allow(non_snake_case)]
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub const PROFILE_BALANCED: &str = "balanced";
pub const PROFILE_CORE: &str = "core";
pub const PROFILE_SURVIVAL: &str = "survival";

pub fn get_profile_label(profile: &str) -> &'static str {
    match profile {
        PROFILE_BALANCED => "均衡收益",
        PROFILE_CORE => "核心效果优先",
        PROFILE_SURVIVAL => "生存优先",
        _ => "未知策略",
    }
}

pub fn stat_name_en(zh: &str) -> Option<&'static str> {
    match zh {
        "力量" => Some("Strength"),
        "敏捷" => Some("Dexterity"),
        "智力" => Some("Intelligence"),
        "意力" => Some("Willpower"),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOptions {
    pub profile: String,
    pub glyph_rank: Option<u32>,
    pub glyph_ranks: HashMap<String, u32>,
    pub time_limit: f64,
}

impl Default for PlannerOptions {
    fn default() -> Self {
        Self {
            profile: PROFILE_BALANCED.to_string(),
            glyph_rank: None,
            glyph_ranks: HashMap::new(),
            time_limit: 5.0,
        }
    }
}

impl PlannerOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.profile != PROFILE_BALANCED && self.profile != PROFILE_CORE && self.profile != PROFILE_SURVIVAL {
            return Err("未知优化策略".to_string());
        }
        if let Some(rank) = self.glyph_rank {
            if rank > 200 {
                return Err("雕文等级必须为 0–200".to_string());
            }
        }
        for (&_, &rank) in &self.glyph_ranks {
            if rank > 200 {
                return Err("雕文等级必须为 0–200".to_string());
            }
        }
        if !self.time_limit.is_finite() || self.time_limit <= 0.0 {
            return Err("求解时间必须大于零".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct GlyphRequirement {
    pub name: String,
    pub required: i64,
    pub amounts: HashMap<String, i64>,
}

#[derive(Debug, Clone)]
pub struct GlyphModel {
    pub ref_key: String,
    pub name: String,
    pub rank: u32,
    pub radius: usize,
    pub requirements: Vec<GlyphRequirement>,
    pub scaling: HashMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementDetail {
    pub name: String,
    pub required: f64,
    pub actual: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphDetail {
    pub name: String,
    pub rank: u32,
    pub radius: usize,
    pub socketSelected: bool,
    pub active: bool,
    pub requirements: Vec<RequirementDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub score: i64,
    pub legendaryCount: usize,
    pub activeGlyphCount: usize,
    pub lifePercent: f64,
    pub glyphs: Vec<GlyphDetail>,
    pub profile: String,
    pub profileLabel: String,
    pub status: String,
    pub optimal: bool,
    pub objective: f64,
    pub upperBound: f64,
    pub seconds: f64,
    pub budget: usize,
    pub unspentPoints: usize,
    pub assumptions: Vec<String>,
}
