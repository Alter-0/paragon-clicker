use crate::d2core::model::Step;
use super::types::PROFILE_SURVIVAL;

pub fn node_score(step: &Step, _char: &str, profile: &str) -> i64 {
    let mut score = if step.nodeKind == "legendary" { 12000.0 } else { 0.0 };
    let defense = if profile == PROFILE_SURVIVAL { 3.0 } else { 1.0 };

    for attr in &step.attributes {
        let name = &attr.name;
        let value = attr.value;
        let weight = if name == "Strength" || name == "Dexterity" || name == "Intelligence" || name == "Willpower" {
            2.0
        } else if name.contains("HPMaxBonus") {
            250.0 * defense
        } else if name.contains("ResistAll") {
            80.0 * defense
        } else if name.contains("ArmorBonus") {
            0.5 * defense
        } else if name.contains("Damage") {
            20.0
        } else if name.contains("CritChance") || name.contains("AttackSpeed") {
            100.0
        } else if name.contains("#161#") || name.contains("Resource") {
            40.0
        } else {
            1.0
        };
        score += (value * weight).round();
    }

    (score as i64).max(0)
}
