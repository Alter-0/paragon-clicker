#![allow(non_snake_case)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Coord {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Attribute {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThresholdRequirement {
    pub name: Option<String>,
    pub raw: String,
    pub resolved: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepGlyph {
    pub key: String,
    pub name: Option<String>,
    pub rank: u32,
    pub definition: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectedNode {
    pub key: String,
    pub nodeId: String,
    pub row: usize,
    pub col: usize,
    pub rotated: Coord,
    pub kind: String,
    pub name: String,
    pub desc: Option<String>,
    pub connected: bool,
    pub pointOrder: Option<usize>,
    pub glyph: Option<String>,
    pub glyphRank: Option<u32>,
    pub thresholds: Vec<ThresholdRequirement>,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Board {
    pub boardKey: String,
    pub boardName: String,
    pub index: i32,
    pub rotate: i32,
    pub parent: Option<String>,
    pub position: Position,
    pub selectedNodes: Vec<SelectedNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Step {
    pub step: usize,
    pub localStep: Option<usize>,
    pub action: String,
    pub boardKey: String,
    pub boardName: String,
    pub boardIndex: i32,
    pub boardPosition: Position,
    pub boardRotate: i32,
    pub parentBoardKey: Option<String>,
    pub nodeKey: String,
    pub nodeId: String,
    pub nodeName: String,
    pub nodeKind: String,
    pub rawCoord: Coord,
    pub rotatedCoord: Coord,
    pub connected: bool,
    pub glyph: Option<StepGlyph>,
    pub thresholds: Vec<ThresholdRequirement>,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntryNode {
    pub nodeKey: String,
    pub nodeName: String,
    pub nodeKind: String,
    pub rawCoord: Coord,
    pub rotatedCoord: Coord,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoardSequence {
    pub boardSequenceIndex: usize,
    pub boardKey: String,
    pub boardName: String,
    pub boardIndex: i32,
    pub boardPosition: Position,
    pub boardRotate: i32,
    pub parentBoardKey: Option<String>,
    pub clickCount: usize,
    pub entryNodes: Vec<EntryNode>,
    pub steps: Vec<Step>,
}

impl BoardSequence {
    pub fn label(&self) -> String {
        format!("{}: {} ({})", self.boardIndex, self.boardName, self.boardKey)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoardOrderEntry {
    pub order: usize,
    pub boardKey: String,
    pub boardName: String,
    pub boardIndex: i32,
    pub boardPosition: Position,
    pub boardRotate: i32,
    pub parentBoardKey: Option<String>,
    pub firstStep: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoardFlowEntry {
    pub segment: usize,
    pub boardKey: String,
    pub boardName: String,
    pub boardIndex: i32,
    pub boardPosition: Position,
    pub boardRotate: i32,
    pub parentBoardKey: Option<String>,
    pub firstStep: usize,
    pub lastStep: usize,
    pub clickCount: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VariantMeta {
    pub title: Option<String>,
    pub char: Option<String>,
    pub season: Option<serde_json::Value>,
    pub variantIndex: usize,
    pub variantName: Option<String>,
    pub boardCount: usize,
    pub pointCount: usize,
    pub nodeCount: usize,
    pub freeNodeCount: usize,
    pub fullPointCount: Option<usize>,
    pub fullNodeCount: Option<usize>,
    pub availablePointCount: Option<usize>,
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VariantSequence {
    pub meta: VariantMeta,
    pub mode: String,
    pub boardOrder: Vec<BoardOrderEntry>,
    pub boardSequences: Vec<BoardSequence>,
    pub steps: Vec<Step>,
    pub globalBoardFlow: Vec<BoardFlowEntry>,
    pub globalSteps: Vec<Step>,
    #[serde(default)]
    pub plannedGlobalSteps: Option<Vec<Step>>,
    #[serde(default)]
    pub optimization: Option<serde_json::Value>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlannerMeta {
    pub bd: String,
    pub title: Option<String>,
    pub char: Option<String>,
    pub season: Option<serde_json::Value>,
    pub variantCount: usize,
    pub selectedVariantIndex: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlannerInputResult {
    pub meta: PlannerMeta,
    pub variants: Vec<VariantSequence>,
}

#[derive(Debug, Clone)]
pub struct ClickPoint {
    pub step: usize,
    pub local_step: usize,
    pub node_name: String,
    pub node_kind: String,
    pub board_key: String,
    pub row: usize,
    pub col: usize,
    pub x: i32,
    pub y: i32,
}
