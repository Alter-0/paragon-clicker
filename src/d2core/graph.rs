use std::collections::{HashMap, HashSet, VecDeque};
use serde_json::Value;

use super::geometry::{
    fallback_node_name, get_connect_board_pos, get_node_kind, get_parent_entry_edge,
    get_rotated_pos, is_cell_on_edge, parse_node_key, resolve_thresholds, DELTAS, NODE_NUM,
};
use super::model::{
    Attribute, Board, BoardFlowEntry, BoardOrderEntry, BoardSequence, Coord, EntryNode, Position,
    SelectedNode, Step, StepGlyph, VariantMeta, VariantSequence,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StepRef {
    pub board: String,
    pub key: String,
}

pub fn build_step_ref_str(board_key: &str, node_key: &str) -> String {
    format!("{board_key}:{node_key}")
}

pub fn build_step_ref(step: &Step) -> String {
    build_step_ref_str(&step.boardKey, &step.nodeKey)
}

pub fn get_connect_path_with_order(
    spent_map: &HashMap<String, Value>,
) -> (HashMap<String, Vec<String>>, Vec<StepRef>) {
    let mut rotated_boards: HashMap<String, Vec<Vec<Option<String>>>> = HashMap::new();
    let mut visited: HashMap<String, HashMap<String, bool>> = HashMap::new();
    let mut queue: VecDeque<(String, String, usize, usize)> = VecDeque::new();
    let mut order: Vec<StepRef> = Vec::new();

    for (board_key, board_state) in spent_map {
        visited.insert(board_key.clone(), HashMap::new());
        let mut grid: Vec<Vec<Option<String>>> = vec![vec![None; NODE_NUM]; NODE_NUM];
        let rotate = board_state.get("rotate").and_then(|r| r.as_i64()).unwrap_or(0) as i32;

        if let Some(data) = board_state.get("data").and_then(|d| d.as_array()) {
            for node_val in data {
                if let Some(node_key) = node_val.as_str() {
                    let (row, col, _) = parse_node_key(node_key);
                    let rotated = get_rotated_pos(row, col, rotate);
                    grid[rotated.row][rotated.col] = Some(node_key.to_string());

                    if node_key.contains("StartNode") {
                        visited.get_mut(board_key).unwrap().insert(node_key.to_string(), true);
                        queue.push_back((
                            board_key.clone(),
                            node_key.to_string(),
                            rotated.row,
                            rotated.col,
                        ));
                        order.push(StepRef {
                            board: board_key.clone(),
                            key: node_key.to_string(),
                        });
                    }
                }
            }
        }
        rotated_boards.insert(board_key.clone(), grid);
    }

    while let Some((curr_board, curr_key, curr_row, curr_col)) = queue.pop_front() {
        for (dy, dx) in DELTAS {
            let next_row_i = curr_row as i32 + dy;
            let next_col_i = curr_col as i32 + dx;
            let mut next_board = curr_board.clone();
            let mut next_key: Option<String> = None;
            let mut final_row = 0;
            let mut final_col = 0;

            if next_row_i >= 0 && next_row_i < NODE_NUM as i32 && next_col_i >= 0 && next_col_i < NODE_NUM as i32 {
                let r = next_row_i as usize;
                let c = next_col_i as usize;
                if let Some(ref k) = rotated_boards.get(&next_board).unwrap()[r][c] {
                    next_key = Some(k.clone());
                    final_row = r;
                    final_col = c;
                }
            }

            if next_key.is_none() && curr_key.contains("Generic_Gate") {
                if let Some(board_state) = spent_map.get(&curr_board) {
                    let bx = board_state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let by = board_state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let (target_x, target_y, target_row, target_col) =
                        get_connect_board_pos(bx, by, curr_row, curr_col);

                    for (candidate_key, candidate_state) in spent_map {
                        let cx = candidate_state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let cy = candidate_state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        if cx == target_x && cy == target_y {
                            next_board = candidate_key.clone();
                            final_row = target_row;
                            final_col = target_col;
                            if let Some(ref k) = rotated_boards.get(&next_board).unwrap()[final_row][final_col] {
                                next_key = Some(k.clone());
                            }
                            break;
                        }
                    }
                }
            }

            if let Some(ref k) = next_key {
                let is_visited = visited
                    .get(&next_board)
                    .and_then(|m| m.get(k))
                    .copied()
                    .unwrap_or(false);
                if !is_visited {
                    visited.get_mut(&next_board).unwrap().insert(k.clone(), true);
                    queue.push_back((next_board.clone(), k.clone(), final_row, final_col));
                    order.push(StepRef {
                        board: next_board,
                        key: k.clone(),
                    });
                }
            }
        }
    }

    let mut connect_path = HashMap::new();
    for (board_key, nodes) in visited {
        connect_path.insert(board_key, nodes.keys().cloned().collect());
    }
    (connect_path, order)
}

pub fn get_node_definition<'a>(
    paragon_db: &'a Value,
    char: &str,
    node_id: &str,
) -> Option<&'a Value> {
    paragon_db
        .get("Generic")
        .and_then(|g| g.get("node"))
        .and_then(|n| n.get(node_id))
        .or_else(|| {
            paragon_db
                .get(char)
                .and_then(|c| c.get("node"))
                .and_then(|n| n.get(node_id))
        })
}

pub fn build_variant_boards(
    variant: &Value,
    char: &str,
    paragon_db: &Value,
) -> Result<(Vec<Board>, Vec<StepRef>), String> {
    let empty_map = serde_json::Map::new();
    let spent_map_json = variant.get("paragon").and_then(|p| p.as_object()).unwrap_or(&empty_map);
    let mut spent_map: HashMap<String, Value> = HashMap::new();
    for (k, v) in spent_map_json {
        spent_map.insert(k.clone(), v.clone());
    }

    let (connect_path, order) = get_connect_path_with_order(&spent_map);
    let mut order_map: HashMap<String, usize> = HashMap::new();
    for (idx, item) in order.iter().enumerate() {
        order_map.insert(format!("{}:{}", item.board, item.key), idx);
    }

    let mut board_keys: Vec<String> = spent_map.keys().cloned().collect();
    board_keys.sort_by_key(|k| {
        spent_map.get(k).and_then(|s| s.get("index")).and_then(|i| i.as_i64()).unwrap_or(0)
    });

    let mut boards = Vec::new();
    for board_key in board_keys {
        let board_state = spent_map.get(&board_key).unwrap();
        let board_name = paragon_db
            .get(char)
            .and_then(|c| c.get("board"))
            .and_then(|b| b.get(&board_key))
            .and_then(|item| item.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(&board_key)
            .to_string();

        let rotate = board_state.get("rotate").and_then(|r| r.as_i64()).unwrap_or(0) as i32;
        let board_idx = board_state.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32;
        let parent = board_state.get("parent").and_then(|p| p.as_str()).map(|s| s.to_string());
        let pos_x = board_state.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let pos_y = board_state.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        let mut selected_nodes = Vec::new();
        if let Some(data) = board_state.get("data").and_then(|d| d.as_array()) {
            for node_val in data {
                if let Some(node_key) = node_val.as_str() {
                    let (row, col, node_id) = parse_node_key(node_key);
                    let node_def = get_node_definition(paragon_db, char, &node_id);
                    let rotated = get_rotated_pos(row, col, rotate);
                    let kind = get_node_kind(&node_id);
                    let name = node_def
                        .and_then(|d| d.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| fallback_node_name(&node_id));
                    let desc = node_def
                        .and_then(|d| d.get("desc"))
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    let connected = connect_path
                        .get(&board_key)
                        .map(|list| list.contains(&node_key.to_string()))
                        .unwrap_or(false);
                    let point_order = order_map.get(&format!("{board_key}:{node_key}")).copied();

                    let glyph_key = board_state
                        .get("glyph")
                        .and_then(|g| g.get(node_key))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let glyph_rank = board_state
                        .get("glyphRank")
                        .and_then(|gr| gr.get(node_key))
                        .and_then(|v| v.as_u64())
                        .map(|u| u as u32);

                    let thresholds = resolve_thresholds(node_def, char, board_idx);
                    let mut attributes = Vec::new();
                    if let Some(attrs) = node_def.and_then(|d| d.get("attributes")).and_then(|a| a.as_array()) {
                        for attr_val in attrs {
                            if let (Some(name), Some(val)) = (
                                attr_val.get("name").and_then(|n| n.as_str()),
                                attr_val.get("value").and_then(|v| v.as_f64()),
                            ) {
                                attributes.push(Attribute {
                                    name: name.to_string(),
                                    value: val,
                                });
                            }
                        }
                    }

                    selected_nodes.push(SelectedNode {
                        key: node_key.to_string(),
                        nodeId: node_id,
                        row,
                        col,
                        rotated,
                        kind,
                        name,
                        desc,
                        connected,
                        pointOrder: point_order,
                        glyph: glyph_key,
                        glyphRank: glyph_rank,
                        thresholds,
                        attributes,
                    });
                }
            }
        }

        selected_nodes.sort_by_key(|item| {
            (item.pointOrder.is_none(), item.pointOrder.unwrap_or(1_000_000_000))
        });

        boards.push(Board {
            boardKey: board_key,
            boardName: board_name,
            index: board_idx,
            rotate,
            parent,
            position: Position { x: pos_x, y: pos_y },
            selectedNodes: selected_nodes,
        });
    }

    Ok((boards, order))
}

pub fn get_board_entry_cells(
    board: &Board,
    boards_by_key: &HashMap<String, &Board>,
) -> Vec<SelectedNode> {
    if board.parent.is_none() {
        let starts: Vec<SelectedNode> = board
            .selectedNodes
            .iter()
            .filter(|c| c.kind == "start")
            .cloned()
            .collect();
        if !starts.is_empty() {
            return starts;
        }
    }

    let parent_edge = get_parent_entry_edge(board, boards_by_key);
    let parent_gates: Vec<SelectedNode> = board
        .selectedNodes
        .iter()
        .filter(|c| c.kind == "gate" && is_cell_on_edge(c, parent_edge))
        .cloned()
        .collect();
    if !parent_gates.is_empty() {
        return parent_gates;
    }

    let any_gates: Vec<SelectedNode> = board
        .selectedNodes
        .iter()
        .filter(|c| c.kind == "gate")
        .cloned()
        .collect();
    if !any_gates.is_empty() {
        return any_gates;
    }

    board.selectedNodes.iter().take(1).cloned().collect()
}

pub fn build_board_local_cells(
    board: &Board,
    boards_by_key: &HashMap<String, &Board>,
) -> (Vec<SelectedNode>, Vec<SelectedNode>) {
    let mut selected_map: HashMap<(usize, usize), SelectedNode> = HashMap::new();
    for cell in &board.selectedNodes {
        selected_map.insert((cell.rotated.row, cell.rotated.col), cell.clone());
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<SelectedNode> = VecDeque::new();
    let mut result: Vec<SelectedNode> = Vec::new();

    let entry_cells = get_board_entry_cells(board, boards_by_key);
    for cell in &entry_cells {
        if !visited.contains(&cell.key) {
            visited.insert(cell.key.clone());
            queue.push_back(cell.clone());
        }
    }

    while !queue.is_empty() || visited.len() < board.selectedNodes.len() {
        while let Some(current) = queue.pop_front() {
            result.push(current.clone());
            for (dy, dx) in DELTAS {
                let nr = current.rotated.row as i32 + dy;
                let nc = current.rotated.col as i32 + dx;
                if nr >= 0 && nr < NODE_NUM as i32 && nc >= 0 && nc < NODE_NUM as i32 {
                    if let Some(neighbor) = selected_map.get(&(nr as usize, nc as usize)) {
                        if !visited.contains(&neighbor.key) {
                            visited.insert(neighbor.key.clone());
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        if visited.len() < board.selectedNodes.len() {
            if let Some(seed) = board.selectedNodes.iter().find(|c| !visited.contains(&c.key)) {
                visited.insert(seed.key.clone());
                queue.push_back(seed.clone());
            }
        }
    }

    (entry_cells, result)
}

pub fn build_step(
    board: &Board,
    cell: &SelectedNode,
    step_num: usize,
    local_step: Option<usize>,
    glyphs: &Value,
) -> Step {
    let mut glyph = None;
    if let Some(ref gkey) = cell.glyph {
        let def = glyphs.get(gkey).cloned().unwrap_or(Value::Null);
        let name = def.get("name").and_then(|n| n.as_str()).map(|s| s.to_string());
        glyph = Some(StepGlyph {
            key: gkey.clone(),
            name,
            rank: cell.glyphRank.unwrap_or(0),
            definition: def,
        });
    }

    Step {
        step: step_num,
        localStep: local_step,
        action: "click_node".to_string(),
        boardKey: board.boardKey.clone(),
        boardName: board.boardName.clone(),
        boardIndex: board.index,
        boardPosition: board.position.clone(),
        boardRotate: board.rotate,
        parentBoardKey: board.parent.clone(),
        nodeKey: cell.key.clone(),
        nodeId: cell.nodeId.clone(),
        nodeName: cell.name.clone(),
        nodeKind: cell.kind.clone(),
        rawCoord: Coord { row: cell.row, col: cell.col },
        rotatedCoord: cell.rotated.clone(),
        connected: cell.connected,
        glyph,
        thresholds: cell.thresholds.clone(),
        attributes: cell.attributes.clone(),
    }
}

pub fn build_board_order(steps: &[Step]) -> Vec<BoardOrderEntry> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for step in steps {
        if seen.contains(&step.boardKey) {
            continue;
        }
        seen.insert(step.boardKey.clone());
        order.push(BoardOrderEntry {
            order: order.len() + 1,
            boardKey: step.boardKey.clone(),
            boardName: step.boardName.clone(),
            boardIndex: step.boardIndex,
            boardPosition: step.boardPosition.clone(),
            boardRotate: step.boardRotate,
            parentBoardKey: step.parentBoardKey.clone(),
            firstStep: step.step,
        });
    }
    order
}

pub fn build_board_flow(steps: &[Step]) -> Vec<BoardFlowEntry> {
    let mut groups: Vec<BoardFlowEntry> = Vec::new();
    for step in steps {
        let is_same_board = groups.last().map(|g| g.boardKey == step.boardKey).unwrap_or(false);
        if !is_same_board {
            groups.push(BoardFlowEntry {
                segment: groups.len() + 1,
                boardKey: step.boardKey.clone(),
                boardName: step.boardName.clone(),
                boardIndex: step.boardIndex,
                boardPosition: step.boardPosition.clone(),
                boardRotate: step.boardRotate,
                parentBoardKey: step.parentBoardKey.clone(),
                firstStep: step.step,
                lastStep: step.step,
                clickCount: 1,
            });
        } else {
            let last = groups.last_mut().unwrap();
            last.lastStep = step.step;
            last.clickCount += 1;
        }
    }
    groups
}

pub fn get_free_step_refs_from_board_sequences(
    board_sequences: &[BoardSequence],
) -> Result<HashSet<String>, String> {
    let mut free_refs = HashSet::new();
    for sequence in board_sequences {
        if sequence.steps.is_empty() {
            continue;
        }
        let entries: HashSet<String> = sequence.entryNodes.iter().map(|n| n.nodeKey.clone()).collect();
        let candidates: Vec<&Step> = sequence
            .steps
            .iter()
            .filter(|step| {
                step.nodeKind == "start"
                    || (sequence.parentBoardKey.is_some()
                        && step.nodeKind == "gate"
                        && entries.contains(&step.nodeKey))
            })
            .collect();

        if candidates.len() != 1 {
            return Err(format!("板块 {} 缺少唯一的起点或父板入口", sequence.boardKey));
        }
        free_refs.insert(build_step_ref(candidates[0]));
    }
    Ok(free_refs)
}

pub fn build_variant_sequence(
    root_data: &Value,
    variant: &Value,
    paragon_db: &Value,
) -> Result<VariantSequence, String> {
    let char = root_data
        .get("char")
        .and_then(|c| c.as_str())
        .or_else(|| variant.get("char").and_then(|c| c.as_str()))
        .ok_or_else(|| "Missing character type in build data".to_string())?;

    let (boards, global_order) = build_variant_boards(variant, char, paragon_db)?;
    let mut boards_by_key: HashMap<String, Board> = HashMap::new();
    for b in boards {
        boards_by_key.insert(b.boardKey.clone(), b);
    }

    let empty_obj = json_map_empty();
    let glyphs = paragon_db
        .get(char)
        .and_then(|c| c.get("glyph"))
        .unwrap_or(&empty_obj);

    let mut global_steps = Vec::new();
    for (index, point) in global_order.iter().enumerate() {
        let board = boards_by_key.get(&point.board).unwrap();
        let cell = board
            .selectedNodes
            .iter()
            .find(|c| c.key == point.key)
            .unwrap();
        global_steps.push(build_step(board, cell, index + 1, None, glyphs));
    }

    let mut step_counter = 1;
    let mut board_sequences = Vec::new();
    let mut sorted_boards: Vec<&Board> = boards_by_key.values().collect();
    sorted_boards.sort_by_key(|b| b.index);

    let boards_ref_map: HashMap<String, &Board> = boards_by_key.iter().map(|(k, v)| (k.clone(), v)).collect();

    for (seq_idx, board) in sorted_boards.iter().enumerate() {
        let (entry_cells, cells) = build_board_local_cells(board, &boards_ref_map);
        let mut steps = Vec::new();
        for (local_idx, cell) in cells.iter().enumerate() {
            steps.push(build_step(board, cell, step_counter, Some(local_idx + 1), glyphs));
            step_counter += 1;
        }

        board_sequences.push(BoardSequence {
            boardSequenceIndex: seq_idx + 1,
            boardKey: board.boardKey.clone(),
            boardName: board.boardName.clone(),
            boardIndex: board.index,
            boardPosition: board.position.clone(),
            boardRotate: board.rotate,
            parentBoardKey: board.parent.clone(),
            clickCount: steps.len(),
            entryNodes: entry_cells
                .iter()
                .map(|c| EntryNode {
                    nodeKey: c.key.clone(),
                    nodeName: c.name.clone(),
                    nodeKind: c.kind.clone(),
                    rawCoord: Coord { row: c.row, col: c.col },
                    rotatedCoord: c.rotated.clone(),
                })
                .collect(),
            steps,
        });
    }

    let flat_steps: Vec<Step> = board_sequences.iter().flat_map(|s| s.steps.clone()).collect();
    let free_refs = get_free_step_refs_from_board_sequences(&board_sequences)?;
    let spent_point_count = flat_steps
        .iter()
        .filter(|s| !free_refs.contains(&build_step_ref(s)))
        .count();

    let var_idx = variant.get("variantIndex").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let var_name = variant.get("name").and_then(|n| n.as_str()).map(|s| s.to_string());

    Ok(VariantSequence {
        meta: VariantMeta {
            title: root_data.get("title").and_then(|t| t.as_str()).map(|s| s.to_string()),
            char: Some(char.to_string()),
            season: root_data.get("season").cloned(),
            variantIndex: var_idx,
            variantName: var_name,
            boardCount: boards_by_key.len(),
            pointCount: spent_point_count,
            nodeCount: global_steps.len(),
            freeNodeCount: free_refs.len(),
            fullPointCount: None,
            fullNodeCount: None,
            availablePointCount: None,
            strategy: None,
        },
        mode: "board_by_board".to_string(),
        boardOrder: build_board_order(&flat_steps),
        boardSequences: board_sequences,
        steps: flat_steps,
        globalBoardFlow: build_board_flow(&global_steps),
        globalSteps: global_steps,
        plannedGlobalSteps: None,
        optimization: None,
        notes: vec![
            "steps is the recommended board-by-board click order without board switching automation.".to_string(),
            "boardSequences contains the per-board click list after you manually switch to that board.".to_string(),
            "The selected screen rectangle is divided into 21x21 cells and the program clicks the center of each target cell.".to_string(),
        ],
    })
}

fn json_map_empty() -> Value {
    Value::Object(serde_json::Map::new())
}

pub fn build_step_graph(
    variant_sequence: &VariantSequence,
) -> (
    HashMap<String, Step>,
    HashMap<String, HashSet<String>>,
    Option<String>,
) {
    let steps = &variant_sequence.globalSteps;
    let mut node_map: HashMap<String, Step> = HashMap::new();
    for step in steps {
        node_map.insert(build_step_ref(step), step.clone());
    }

    let mut adjacency: HashMap<String, HashSet<String>> = HashMap::new();
    for ref_str in node_map.keys() {
        adjacency.insert(ref_str.clone(), HashSet::new());
    }

    let mut coord_map: HashMap<String, HashMap<(usize, usize), String>> = HashMap::new();
    for (ref_str, step) in &node_map {
        coord_map
            .entry(step.boardKey.clone())
            .or_default()
            .insert((step.rotatedCoord.row, step.rotatedCoord.col), ref_str.clone());
    }

    for (ref_str, step) in &node_map {
        let board_key = &step.boardKey;
        let row = step.rotatedCoord.row;
        let col = step.rotatedCoord.col;

        for (dy, dx) in DELTAS {
            let nr = row as i32 + dy;
            let nc = col as i32 + dx;
            if nr >= 0 && nr < NODE_NUM as i32 && nc >= 0 && nc < NODE_NUM as i32 {
                if let Some(n_ref) = coord_map
                    .get(board_key)
                    .and_then(|m| m.get(&(nr as usize, nc as usize)))
                {
                    adjacency.get_mut(ref_str).unwrap().insert(n_ref.clone());
                }
            }
        }

        if step.nodeKind == "gate" {
            let (target_x, target_y, target_row, target_col) =
                get_connect_board_pos(step.boardPosition.x, step.boardPosition.y, row, col);

            for (cand_ref, cand_step) in &node_map {
                if cand_step.boardPosition.x == target_x
                    && cand_step.boardPosition.y == target_y
                    && cand_step.rotatedCoord.row == target_row
                    && cand_step.rotatedCoord.col == target_col
                {
                    adjacency.get_mut(ref_str).unwrap().insert(cand_ref.clone());
                    adjacency.get_mut(cand_ref).unwrap().insert(ref_str.clone());
                    break;
                }
            }
        }
    }

    let root_ref = steps.first().map(build_step_ref);
    (node_map, adjacency, root_ref)
}

pub fn build_variant_from_planned_steps(
    template_variant: &VariantSequence,
    selected_steps: &[Step],
    available_point_count: usize,
) -> VariantSequence {
    let mut board_template_map: HashMap<String, Board> = HashMap::new();
    for step in &template_variant.globalSteps {
        board_template_map.entry(step.boardKey.clone()).or_insert_with(|| Board {
            boardKey: step.boardKey.clone(),
            boardName: step.boardName.clone(),
            index: step.boardIndex,
            rotate: step.boardRotate,
            parent: step.parentBoardKey.clone(),
            position: step.boardPosition.clone(),
            selectedNodes: Vec::new(),
        });
    }

    let mut ordered_steps: Vec<Step> = Vec::new();
    for (idx, step) in selected_steps.iter().enumerate() {
        let mut ordered_step = step.clone();
        ordered_step.step = idx + 1;
        ordered_steps.push(ordered_step);

        let board = board_template_map.get_mut(&step.boardKey).unwrap();
        board.selectedNodes.push(SelectedNode {
            key: step.nodeKey.clone(),
            nodeId: step.nodeId.clone(),
            row: step.rawCoord.row,
            col: step.rawCoord.col,
            rotated: step.rotatedCoord.clone(),
            kind: step.nodeKind.clone(),
            name: step.nodeName.clone(),
            desc: None,
            connected: step.connected,
            pointOrder: Some(idx),
            glyph: step.glyph.as_ref().map(|g| g.key.clone()),
            glyphRank: step.glyph.as_ref().map(|g| g.rank),
            thresholds: step.thresholds.clone(),
            attributes: step.attributes.clone(),
        });
    }

    let mut boards: Vec<Board> = board_template_map
        .into_values()
        .filter(|b| !b.selectedNodes.is_empty())
        .collect();
    boards.sort_by_key(|b| b.index);

    let mut boards_by_key: HashMap<String, Board> = HashMap::new();
    for b in &boards {
        boards_by_key.insert(b.boardKey.clone(), b.clone());
    }

    let mut step_counter = 1;
    let mut board_sequences = Vec::new();
    let boards_ref_map: HashMap<String, &Board> = boards_by_key.iter().map(|(k, v)| (k.clone(), v)).collect();

    for (seq_idx, board) in boards.iter().enumerate() {
        let (entry_cells, cells) = build_board_local_cells(board, &boards_ref_map);
        let mut board_steps = Vec::new();
        for (local_idx, cell) in cells.iter().enumerate() {
            let ref_str = build_step_ref_str(&board.boardKey, &cell.key);
            let matching = ordered_steps.iter().find(|s| build_step_ref(s) == ref_str).unwrap();
            let mut planned_step = matching.clone();
            planned_step.step = step_counter;
            planned_step.localStep = Some(local_idx + 1);
            board_steps.push(planned_step);
            step_counter += 1;
        }

        board_sequences.push(BoardSequence {
            boardSequenceIndex: seq_idx + 1,
            boardKey: board.boardKey.clone(),
            boardName: board.boardName.clone(),
            boardIndex: board.index,
            boardPosition: board.position.clone(),
            boardRotate: board.rotate,
            parentBoardKey: board.parent.clone(),
            clickCount: board_steps.len(),
            entryNodes: entry_cells
                .iter()
                .map(|c| EntryNode {
                    nodeKey: c.key.clone(),
                    nodeName: c.name.clone(),
                    nodeKind: c.kind.clone(),
                    rawCoord: Coord { row: c.row, col: c.col },
                    rotatedCoord: c.rotated.clone(),
                })
                .collect(),
            steps: board_steps,
        });
    }

    let flat_steps: Vec<Step> = board_sequences.iter().flat_map(|s| s.steps.clone()).collect();
    let free_refs = get_free_step_refs_from_board_sequences(&board_sequences).unwrap_or_default();
    let spent_point_count = flat_steps
        .iter()
        .filter(|s| !free_refs.contains(&build_step_ref(s)))
        .count();

    let full_point_count = template_variant.meta.pointCount;
    let full_node_count = template_variant.meta.nodeCount;

    let mut result = template_variant.clone();
    result.meta.pointCount = spent_point_count;
    result.meta.nodeCount = ordered_steps.len();
    result.meta.fullPointCount = Some(full_point_count);
    result.meta.fullNodeCount = Some(full_node_count);
    result.meta.availablePointCount = Some(available_point_count);
    result.meta.freeNodeCount = free_refs.len();
    result.meta.strategy = Some("budget_connected_optimization".to_string());

    result.boardOrder = build_board_order(&flat_steps);
    result.boardSequences = board_sequences;
    result.steps = flat_steps;
    result.globalSteps = ordered_steps.clone();
    result.plannedGlobalSteps = Some(ordered_steps.clone());
    result.globalBoardFlow = build_board_flow(&ordered_steps);

    result
}

pub fn build_sequence_from_planner_input(planner_input: &str) -> Result<super::model::PlannerInputResult, String> {
    use super::api::{fetch_paragon_db, query_plan};
    use super::parser::parse_planner_input;

    let (bd, variant_index) = parse_planner_input(planner_input)?;
    let response = query_plan(&bd)?;
    let root_data = response.get("data").ok_or_else(|| "Planner response does not contain data".to_string())?;

    let default_variants = vec![root_data.clone()];
    let variant_list = root_data
        .get("variants")
        .and_then(|v| v.as_array())
        .unwrap_or(&default_variants);

    if variant_index >= variant_list.len() {
        return Err("链接中的变体编号 var 超出范围".to_string());
    }

    let paragon_db = fetch_paragon_db(None, None)?;
    let mut sequences = Vec::new();

    for (index, variant) in variant_list.iter().enumerate() {
        let mut tagged = variant.clone();
        if let Some(obj) = tagged.as_object_mut() {
            obj.insert("variantIndex".to_string(), serde_json::Value::from(index));
        }
        let seq = build_variant_sequence(root_data, &tagged, &paragon_db)?;
        sequences.push(seq);
    }

    Ok(super::model::PlannerInputResult {
        meta: super::model::PlannerMeta {
            bd,
            title: root_data.get("title").and_then(|t| t.as_str()).map(|s| s.to_string()),
            char: root_data.get("char").and_then(|c| c.as_str()).map(|s| s.to_string()),
            season: root_data.get("season").cloned(),
            variantCount: sequences.len(),
            selectedVariantIndex: variant_index,
        },
        variants: sequences,
    })
}
