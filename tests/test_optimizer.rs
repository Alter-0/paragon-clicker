use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use paragon_clicker_rust::d2core::graph::{
    build_step_graph, build_step_ref, build_variant_sequence,
    get_free_step_refs_from_board_sequences,
};
use paragon_clicker_rust::d2core::model::VariantSequence;
use paragon_clicker_rust::optimizer::glyph::{
    describe_selection, glyph_models, glyph_radius,
};
use paragon_clicker_rust::optimizer::solver::optimize_progression;
use paragon_clicker_rust::optimizer::types::{
    PlannerOptions, PROFILE_BALANCED, PROFILE_CORE, PROFILE_SURVIVAL,
};
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn load_build(bd: &str) -> (Value, VariantSequence) {
    let path = fixtures_dir().join(format!("{bd}.json"));
    let content = fs::read_to_string(&path).expect("Failed to read fixture");
    let fixture: Value = serde_json::from_str(&content).expect("Failed to parse fixture JSON");
    let variant_seq = build_variant_sequence(
        &fixture["root"],
        &fixture["variant"],
        &fixture["database"],
    )
    .expect("Failed to build variant sequence");
    (fixture, variant_seq)
}

fn small_build() -> VariantSequence {
    let nodes = serde_json::json!({
        "Generic_StartNode": {},
        "Generic_Normal_Will": {"attributes": [{"name": "Willpower", "value": 5.0}]},
        "Generic_Socket": {},
        "Test_Rare_Life": {
            "attributes": [{"name": "ParagonNodeHPMaxBonus#142", "value": 8.0}]
        },
        "Test_Legendary_One": {},
    });

    let database = serde_json::json!({
        "Generic": {"node": nodes},
        "Test": {
            "node": nodes,
            "glyph": {
                "glyph": {
                    "name": "Test glyph",
                    "threshold_requirements": [{"name": "Willpower", "value": 10.0}],
                    "desc": "每购买辐射范围内 5 点意力，提高伤害。",
                },
            },
        },
    });

    let variant = serde_json::json!({
        "paragon": {
            "board": {
                "data": [
                    "0_0_Generic_StartNode",
                    "0_1_Generic_Normal_Will",
                    "0_2_Generic_Socket",
                    "1_2_Generic_Normal_Will",
                    "2_2_Test_Rare_Life",
                    "1_1_Generic_Normal_Will",
                    "1_0_Generic_Normal_Will",
                    "2_0_Test_Legendary_One",
                ],
                "glyph": {"0_2_Generic_Socket": "glyph"},
                "glyphRank": {"0_2_Generic_Socket": 15},
            }
        }
    });

    let root_data = serde_json::json!({"char": "Test"});
    build_variant_sequence(&root_data, &variant, &database).expect("small_build failed")
}

fn is_connected(
    selected: &HashSet<String>,
    adjacency: &std::collections::HashMap<String, HashSet<String>>,
    root: &str,
) -> bool {
    let mut reached = HashSet::new();
    let mut pending = vec![root.to_string()];
    reached.insert(root.to_string());

    while let Some(curr) = pending.pop() {
        if let Some(nbrs) = adjacency.get(&curr) {
            for n in nbrs {
                if selected.contains(n) && !reached.contains(n) {
                    reached.insert(n.clone());
                    pending.push(n.clone());
                }
            }
        }
    }

    &reached == selected
}

fn assert_valid_plan(source: &VariantSequence, planned: &VariantSequence, budget: usize) {
    let (nodes, adjacency, root_opt) = build_step_graph(source);
    let root = root_opt.unwrap();
    let selected: HashSet<String> = planned.globalSteps.iter().map(build_step_ref).collect();
    let free = get_free_step_refs_from_board_sequences(&source.boardSequences).unwrap();

    let node_keys: HashSet<String> = nodes.keys().cloned().collect();
    assert!(selected.is_subset(&node_keys));
    assert!(is_connected(&selected, &adjacency, &root));

    let spent = selected.difference(&free).count();
    assert!(spent <= budget);
    assert_eq!(planned.meta.pointCount, spent);

    // Board-by-board execution order reachability check
    let mut seen = HashSet::new();
    for step in &planned.steps {
        let r = build_step_ref(step);
        if r != root {
            let has_prev = adjacency.get(&r).map(|nbrs| nbrs.iter().any(|p| seen.contains(p))).unwrap_or(false);
            assert!(has_prev, "Step {r} is not reachable from previously clicked steps");
        }
        seen.insert(r);
    }
    assert_eq!(seen, selected);
}

#[test]
fn test_glyph_radius() {
    assert_eq!(glyph_radius(1), 3);
    assert_eq!(glyph_radius(14), 3);
    assert_eq!(glyph_radius(15), 4);
    assert_eq!(glyph_radius(49), 4);
    assert_eq!(glyph_radius(50), 5);
    assert_eq!(glyph_radius(150), 5);
}

#[test]
fn test_small_build_exhaustive_optimum() {
    let variant = small_build();
    let (nodes, adjacency, root_opt) = build_step_graph(&variant);
    let root = root_opt.unwrap();
    let others: Vec<String> = nodes.keys().filter(|k| *k != &root).cloned().collect();

    for profile in [PROFILE_BALANCED, PROFILE_CORE, PROFILE_SURVIVAL] {
        for budget in 0..8 {
            let options = PlannerOptions {
                profile: profile.to_string(),
                time_limit: 3.0,
                ..Default::default()
            };
            let glyphs = glyph_models(&nodes, &options).unwrap();

            #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
            struct ExhaustiveKey {
                legendary: usize,
                active: usize,
                score: i64,
                neg_spent: i64,
            }

            let mut feasible_keys = Vec::new();

            for mask in 0..(1 << others.len()) {
                let mut sel: HashSet<String> = HashSet::new();
                sel.insert(root.clone());
                for (i, r) in others.iter().enumerate() {
                    if (mask & (1 << i)) != 0 {
                        sel.insert(r.clone());
                    }
                }

                let spent = sel.len() - 1;
                if spent <= budget && is_connected(&sel, &adjacency, &root) {
                    let (score, leg_cnt, act_cnt, _, _) =
                        describe_selection(&nodes, &sel, "Test", &glyphs, &options);
                    let key = ExhaustiveKey {
                        legendary: if profile == PROFILE_CORE { leg_cnt } else { 0 },
                        active: if profile == PROFILE_CORE { act_cnt } else { 0 },
                        score,
                        neg_spent: -(spent as i64),
                    };
                    feasible_keys.push(key);
                }
            }

            let expected_max = feasible_keys.into_iter().max().unwrap();

            let planned = optimize_progression(&variant, budget, &options).unwrap();
            assert_valid_plan(&variant, &planned, budget);

            let actual_sel: HashSet<String> = planned.globalSteps.iter().map(build_step_ref).collect();
            let (score, leg_cnt, act_cnt, _, _) =
                describe_selection(&nodes, &actual_sel, "Test", &glyphs, &options);
            let actual_key = ExhaustiveKey {
                legendary: if profile == PROFILE_CORE { leg_cnt } else { 0 },
                active: if profile == PROFILE_CORE { act_cnt } else { 0 },
                score,
                neg_spent: -((actual_sel.len() - 1) as i64),
            };

            assert_eq!(
                actual_key, expected_max,
                "Failed at profile={profile}, budget={budget}"
            );
        }
    }
}

#[test]
fn test_real_builds_benchmark() {
    for bd in ["23tb", "23Bt"] {
        let (fixture, variant) = load_build(bd);
        let (nodes, _, _) = build_step_graph(&variant);

        for &rank in &[Some(1), Some(15), None] {
            let options = PlannerOptions {
                glyph_rank: rank,
                time_limit: 10.0,
                ..Default::default()
            };

            let start = std::time::Instant::now();
            let planned = optimize_progression(&variant, 100, &options).unwrap();
            let duration = start.elapsed();

            println!(
                "Build {bd}, rank {:?}, budget 100 solved in {:.3}ms, score={}",
                rank,
                duration.as_secs_f64() * 1000.0,
                planned.optimization.as_ref().unwrap()["score"]
            );

            assert_valid_plan(&variant, &planned, 100);

            // Compare against legacy baseline
            let glyphs = glyph_models(&nodes, &options).unwrap();
            let legacy_sel_array = fixture["legacySelections"]["100"].as_array().unwrap();
            let legacy_set: HashSet<String> = legacy_sel_array
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            let (baseline_score, _, _, _, _) =
                describe_selection(&nodes, &legacy_set, variant.meta.char.as_deref().unwrap_or(""), &glyphs, &options);

            let planned_score = planned.optimization.as_ref().unwrap()["score"].as_i64().unwrap();
            assert!(
                planned_score >= baseline_score,
                "Planned score {planned_score} should be >= legacy baseline {baseline_score}"
            );
        }
    }
}

#[test]
fn test_zero_and_full_budget() {
    let (_, variant) = load_build("23tb");
    let options = PlannerOptions {
        time_limit: 5.0,
        ..Default::default()
    };

    let plan_zero = optimize_progression(&variant, 0, &options).unwrap();
    assert_valid_plan(&variant, &plan_zero, 0);
    assert_eq!(plan_zero.meta.pointCount, 0);

    let full_points = variant.meta.pointCount;
    let plan_full = optimize_progression(&variant, full_points, &options).unwrap();
    assert_valid_plan(&variant, &plan_full, full_points);
    assert_eq!(plan_full.meta.pointCount, full_points);
}
