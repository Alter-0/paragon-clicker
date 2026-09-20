use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;
use minilp::{ComparisonOp, OptimizationDirection, Problem, Variable};

use crate::d2core::graph::{
    build_step_graph, build_step_ref, build_variant_from_planned_steps,
    get_free_step_refs_from_board_sequences,
};
use crate::d2core::model::{Step, VariantSequence};
use super::glyph::{describe_selection, glyph_models, make_optimization_result};
use super::score::node_score;
use super::types::{PlannerOptions, PROFILE_CORE};

#[derive(Clone, PartialEq, Eq)]
struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    fn new(n: usize) -> Self {
        let n_words = (n + 63) / 64;
        BitSet { words: vec![0; n_words] }
    }
    fn full(n: usize) -> Self {
        let n_words = (n + 63) / 64;
        let mut words = vec![!0u64; n_words];
        let rem = n % 64;
        if rem != 0 {
            words[n_words - 1] = (1u64 << rem) - 1;
        }
        BitSet { words }
    }
    #[inline]
    fn set(&mut self, idx: usize) {
        self.words[idx / 64] |= 1u64 << (idx % 64);
    }
    #[inline]
    fn contains(&self, idx: usize) -> bool {
        (self.words[idx / 64] & (1u64 << (idx % 64))) != 0
    }
    #[inline]
    fn intersect_with(&mut self, other: &BitSet) {
        for (w1, w2) in self.words.iter_mut().zip(&other.words) {
            *w1 &= *w2;
        }
    }
    #[inline]
    fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

pub fn optimize_progression(
    variant: &VariantSequence,
    budget: usize,
    options: &PlannerOptions,
) -> Result<VariantSequence, String> {
    options.validate()?;
    let start_time = Instant::now();

    let (nodes, raw_adjacency, _) = build_step_graph(variant);
    if nodes.is_empty() {
        let mut empty_var = build_variant_from_planned_steps(variant, &[], 0);
        let opt_res = make_optimization_result(
            &nodes, &HashSet::new(), "", &[], options, "OPTIMAL", true, 0.0, 0.0, 0.0, 0, 0,
        );
        empty_var.optimization = Some(serde_json::to_value(opt_res).unwrap());
        return Ok(empty_var);
    }

    let roots: Vec<String> = nodes
        .iter()
        .filter(|(_, step)| step.nodeKind == "start")
        .map(|(r, _)| r.clone())
        .collect();
    if roots.len() != 1 {
        return Err("BD 必须包含且只包含一个起始节点".to_string());
    }
    let root = roots[0].clone();

    let expected: HashSet<String> = variant
        .boardSequences
        .iter()
        .flat_map(|b| b.steps.iter().map(build_step_ref))
        .collect();
    let actual_keys: HashSet<String> = nodes.keys().cloned().collect();
    if actual_keys != expected {
        return Err("BD 包含未连接到起点的节点，请检查原始巅峰盘".to_string());
    }

    // Filtered adjacency: only within same board or from parent board to child board
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for (r, neighbors) in &raw_adjacency {
        let r_board = &nodes[r].boardKey;
        let mut filtered: Vec<String> = neighbors
            .iter()
            .filter(|n| {
                let n_step = &nodes[*n];
                &n_step.boardKey == r_board
                    || n_step.parentBoardKey.as_ref() == Some(r_board)
            })
            .cloned()
            .collect();
        filtered.sort();
        adjacency.insert(r.clone(), filtered);
    }

    let free_set = get_free_step_refs_from_board_sequences(&variant.boardSequences)?;
    let available_points = budget;
    let non_free_count = nodes.len().saturating_sub(free_set.len());
    let effective_budget = available_points.min(non_free_count);

    let char = variant.meta.char.as_deref().unwrap_or("");
    let glyphs = glyph_models(&nodes, options)?;

    // Node ordering with root at index 0
    let mut node_list: Vec<String> = nodes.keys().cloned().collect();
    node_list.sort();
    let root_idx = node_list.iter().position(|r| r == &root).unwrap();
    node_list.swap(0, root_idx);

    let node_to_idx: HashMap<String, usize> = node_list
        .iter()
        .enumerate()
        .map(|(i, r)| (r.clone(), i))
        .collect();

    let n_nodes = node_list.len();

    // Fast return: budget 0
    if effective_budget == 0 {
        let mut sel_set = HashSet::new();
        sel_set.insert(root.clone());
        let root_step = nodes[&root].clone();
        let elapsed = start_time.elapsed().as_secs_f64();
        let opt_res = make_optimization_result(
            &nodes, &sel_set, char, &glyphs, options, "OPTIMAL", true, 0.0, 0.0, elapsed, available_points, free_set.len(),
        );
        let mut res_var = build_variant_from_planned_steps(variant, &[root_step], available_points);
        res_var.optimization = Some(serde_json::to_value(opt_res).unwrap());
        return Ok(res_var);
    }

    // Reachability using BitSet
    let mut reachable_bitset = BitSet::new(n_nodes);
    let mut pending = vec![0usize];
    reachable_bitset.set(0);
    while let Some(curr) = pending.pop() {
        if let Some(nbrs) = adjacency.get(&node_list[curr]) {
            for n in nbrs {
                let n_idx = node_to_idx[n];
                if !reachable_bitset.contains(n_idx) {
                    reachable_bitset.set(n_idx);
                    pending.push(n_idx);
                }
            }
        }
    }

    let reachable: HashSet<String> = (0..n_nodes)
        .filter(|&i| reachable_bitset.contains(i))
        .map(|i| node_list[i].clone())
        .collect();

    // Fast return: full budget
    if effective_budget >= non_free_count {
        let sel_set: HashSet<String> = reachable.clone();
        let (ordered_steps, valid) = build_ordered_steps_bfs(&nodes, &adjacency, &root, &sel_set, &glyphs);
        if valid {
            let elapsed = start_time.elapsed().as_secs_f64();
            let opt_res = make_optimization_result(
                &nodes, &sel_set, char, &glyphs, options, "OPTIMAL", true, 0.0, 0.0, elapsed, available_points, free_set.len(),
            );
            let mut res_var = build_variant_from_planned_steps(variant, &ordered_steps, available_points);
            res_var.optimization = Some(serde_json::to_value(opt_res).unwrap());
            return Ok(res_var);
        }
    }

    // Predecessors indexed by usize
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); n_nodes];
    for (r, nbrs) in &adjacency {
        let u = node_to_idx[r];
        for n in nbrs {
            let v = node_to_idx[n];
            predecessors[v].push(u);
        }
    }

    // Dominators calculation using BitSet
    let mut doms: Vec<BitSet> = vec![BitSet::full(n_nodes); n_nodes];
    doms[0] = BitSet::new(n_nodes);
    doms[0].set(0);

    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..n_nodes {
            if !reachable_bitset.contains(i) {
                continue;
            }
            let parents: Vec<usize> = predecessors[i]
                .iter()
                .copied()
                .filter(|&p| reachable_bitset.contains(p))
                .collect();
            if parents.is_empty() {
                continue;
            }
            let mut inter = doms[parents[0]].clone();
            for &p in &parents[1..] {
                inter.intersect_with(&doms[p]);
            }
            inter.set(i);
            if inter != doms[i] {
                doms[i] = inter;
                changed = true;
            }
        }
    }

    let mut idoms: HashMap<usize, usize> = HashMap::new();
    for i in 1..n_nodes {
        if !reachable_bitset.contains(i) {
            continue;
        }
        let mut best_p: Option<usize> = None;
        let mut best_key: Option<(usize, &str)> = None;
        for j in 0..n_nodes {
            if j != i && doms[i].contains(j) {
                let key = (doms[j].count_ones(), node_list[j].as_str());
                if best_key.is_none() || Some(key) > best_key {
                    best_key = Some(key);
                    best_p = Some(j);
                }
            }
        }
        if let Some(p) = best_p {
            idoms.insert(i, p);
        }
    }

    // Compute dominator subtree sizes for hierarchical branching
    let mut dom_subtree_size = vec![0usize; n_nodes];
    for i in 0..n_nodes {
        let mut count = 0;
        for j in 0..n_nodes {
            if doms[j].contains(i) {
                count += 1;
            }
        }
        dom_subtree_size[i] = count;
    }

    let is_core = options.profile == PROFILE_CORE;
    let m = (n_nodes + 1) as f64;

    // Calculate reward bound for lexicographic objective in core mode
    let mut total_score_bound = nodes.values().map(|s| node_score(s, char, &options.profile)).sum::<i64>();
    for g in &glyphs {
        total_score_bound += 500 + 10000 + g.scaling.values().sum::<i64>();
    }

    let mut prob = Problem::new(OptimizationDirection::Maximize);

    // 1. Node variables x_i
    let mut x_vars: Vec<Variable> = Vec::with_capacity(n_nodes);
    for (i, r) in node_list.iter().enumerate() {
        let is_r_root = i == 0;
        let is_r_reachable = reachable.contains(r);
        let bounds = if is_r_root {
            (1.0, 1.0)
        } else if is_r_reachable {
            (0.0, 1.0)
        } else {
            (0.0, 0.0)
        };

        let base_sc = node_score(&nodes[r], char, &options.profile);
        let mut coeff = (base_sc as f64) * m;
        if !free_set.contains(r) {
            coeff -= 1.0;
        }

        let var = prob.add_var(coeff, bounds);
        x_vars.push(var);
    }

    // 2. Flow variables for each directed edge (with Dominator-guided reverse flow pruning)
    let capacity = (n_nodes - 1) as f64;
    let mut flow_vars: HashMap<(usize, usize), Variable> = HashMap::new();
    let mut outgoing: Vec<Vec<(usize, Variable)>> = vec![Vec::new(); n_nodes];
    let mut incoming: Vec<Vec<(usize, Variable)>> = vec![Vec::new(); n_nodes];

    for (u_ref, nbrs) in &adjacency {
        let u = node_to_idx[u_ref];
        for v_ref in nbrs {
            let v = node_to_idx[v_ref];
            // If v dominates u (every path from root to u must already pass through v),
            // flow along u -> v would only cycle back to v and cannot reach any unreached nodes.
            if u != v && doms[u].contains(v) {
                continue;
            }

            let f_var = prob.add_var(0.0, (0.0, capacity));
            flow_vars.insert((u, v), f_var);
            outgoing[u].push((v, f_var));
            incoming[v].push((u, f_var));

            // f_uv <= capacity * x_u
            prob.add_constraint(&[(f_var, 1.0), (x_vars[u], -capacity)], ComparisonOp::Le, 0.0);
            // f_uv <= capacity * x_v
            prob.add_constraint(&[(f_var, 1.0), (x_vars[v], -capacity)], ComparisonOp::Le, 0.0);
        }
    }

    // 3. Flow conservation
    // At root: sum(outgoing) - sum(incoming) - sum_{i != root} x_i = 0
    {
        let mut terms = Vec::new();
        for (_, f) in &outgoing[0] {
            terms.push((*f, 1.0));
        }
        for (_, f) in &incoming[0] {
            terms.push((*f, -1.0));
        }
        for i in 1..n_nodes {
            terms.push((x_vars[i], -1.0));
        }
        prob.add_constraint(&terms, ComparisonOp::Eq, 0.0);
    }

    // At non-root: sum(incoming) - sum(outgoing) - x_i = 0
    for i in 1..n_nodes {
        let mut terms = Vec::new();
        for (_, f) in &incoming[i] {
            terms.push((*f, 1.0));
        }
        for (_, f) in &outgoing[i] {
            terms.push((*f, -1.0));
        }
        terms.push((x_vars[i], -1.0));
        prob.add_constraint(&terms, ComparisonOp::Eq, 0.0);
    }

    // 4. Dominators constraint: x_i <= x_{idom(i)}
    for (&u, &parent_idx) in &idoms {
        prob.add_constraint(&[(x_vars[u], 1.0), (x_vars[parent_idx], -1.0)], ComparisonOp::Le, 0.0);
    }

    // 5. Budget constraint: sum_{i not in free} x_i <= budget
    {
        let mut budget_terms = Vec::new();
        for (i, r) in node_list.iter().enumerate() {
            if !free_set.contains(r) {
                budget_terms.push((x_vars[i], 1.0));
            }
        }
        prob.add_constraint(&budget_terms, ComparisonOp::Le, effective_budget as f64);
    }

    // 6. Glyphs: Sockets, Activations, Scaling
    let mut activation_vars: Vec<Variable> = Vec::new();
    let mut socket_vars: Vec<Variable> = Vec::new();
    let mut scaling_vars: Vec<Variable> = Vec::new();

    for glyph in &glyphs {
        if glyph.rank == 0 {
            continue;
        }
        let s_idx = node_to_idx[&glyph.ref_key];
        let s_var = prob.add_var(500.0 * m, (0.0, 1.0));
        socket_vars.push(s_var);
        // s_g <= x_{s_idx}
        prob.add_constraint(&[(s_var, 1.0), (x_vars[s_idx], -1.0)], ComparisonOp::Le, 0.0);

        if !glyph.requirements.is_empty() {
            let a_var = prob.add_var(10000.0 * m, (0.0, 1.0));
            activation_vars.push(a_var);

            // a_g <= s_g
            prob.add_constraint(&[(a_var, 1.0), (s_var, -1.0)], ComparisonOp::Le, 0.0);

            // For each requirement: req.required * a_g - sum(amt_v * x_v) <= 0
            for req in &glyph.requirements {
                let mut req_terms = Vec::new();
                req_terms.push((a_var, req.required as f64));
                for (other_ref, amt) in &req.amounts {
                    let o_idx = node_to_idx[other_ref];
                    req_terms.push((x_vars[o_idx], -(*amt as f64)));
                }
                prob.add_constraint(&req_terms, ComparisonOp::Le, 0.0);
            }
        }

        for (other_ref, val) in &glyph.scaling {
            let o_idx = node_to_idx[other_ref];
            let z_var = prob.add_var((*val as f64) * m, (0.0, 1.0));
            scaling_vars.push(z_var);
            // z <= s_g
            prob.add_constraint(&[(z_var, 1.0), (s_var, -1.0)], ComparisonOp::Le, 0.0);
            // z <= x_v
            prob.add_constraint(&[(z_var, 1.0), (x_vars[o_idx], -1.0)], ComparisonOp::Le, 0.0);
        }
    }

    // In Core mode: add bonus for legendary nodes and glyph activations
    if is_core {
        let m1 = ((total_score_bound + 1) as f64) * m;
        let m2 = m1 * ((activation_vars.len() + 1) as f64);

        for (i, r) in node_list.iter().enumerate() {
            if nodes[r].nodeKind == "legendary" {
                // Add variable with m2 coefficient, or dummy var constrained to x_vars[i]
                let leg_bonus = prob.add_var(m2, (0.0, 1.0));
                prob.add_constraint(&[(leg_bonus, 1.0), (x_vars[i], -1.0)], ComparisonOp::Eq, 0.0);
            }
        }
        for a_var in &activation_vars {
            let act_bonus = prob.add_var(m1, (0.0, 1.0));
            prob.add_constraint(&[(act_bonus, 1.0), (*a_var, -1.0)], ComparisonOp::Eq, 0.0);
        }
    }

    // Branch and Bound variables with Dominator-Aware priority
    #[derive(Clone, Copy)]
    struct PrioritizedVar {
        var: Variable,
        base_priority: f64,
    }

    let mut decision_vars: Vec<PrioritizedVar> = Vec::new();
    // 1. Activation variables (highest impact: 10000 points)
    for &v in &activation_vars {
        decision_vars.push(PrioritizedVar {
            var: v,
            base_priority: 10_000_000.0,
        });
    }
    // 2. Legendary nodes
    for (i, r) in node_list.iter().enumerate() {
        if nodes[r].nodeKind == "legendary" {
            decision_vars.push(PrioritizedVar {
                var: x_vars[i],
                base_priority: 1_000_000.0,
            });
        }
    }
    // 3. Socket variables
    for &v in &socket_vars {
        decision_vars.push(PrioritizedVar {
            var: v,
            base_priority: 500_000.0,
        });
    }
    // 4. Other reachable nodes: prioritized by dominator subtree size (bottlenecks first) + score
    for (i, r) in node_list.iter().enumerate() {
        if nodes[r].nodeKind != "legendary" && i != 0 && reachable.contains(r) {
            let sc = node_score(&nodes[r], char, &options.profile);
            let sub_sz = dom_subtree_size[i] as f64;
            decision_vars.push(PrioritizedVar {
                var: x_vars[i],
                base_priority: sub_sz * 100.0 + (sc as f64),
            });
        }
    }

    // Branch and Bound search
    let time_limit_dur = std::time::Duration::from_secs_f64(options.time_limit);
    let mut best_obj: f64 = -1e18;
    let mut best_solution_values: Option<Vec<f64>> = None;
    let mut upper_bound_val: f64 = 1e18;
    let mut is_optimal = false;

    let mut stack: Vec<Problem> = Vec::new();
    stack.push(prob);

    let mut nodes_evaluated = 0;

    while let Some(current_prob) = stack.pop() {
        nodes_evaluated += 1;
        if start_time.elapsed() >= time_limit_dur {
            break;
        }

        let sol = match current_prob.solve() {
            Ok(s) => s,
            Err(_) => continue, // Infeasible or error
        };

        if nodes_evaluated == 1 {
            upper_bound_val = sol.objective();

            // Primal heuristic: greedily construct a connected tree using LP relaxation weights
            let mut heuristic_selected: HashSet<usize> = HashSet::new();
            heuristic_selected.insert(0); // root

            let mut current_budget_spent = 0;
            if !free_set.contains(&node_list[0]) {
                current_budget_spent += 1;
            }

            let mut candidate_set: HashSet<usize> = HashSet::new();
            if let Some(nbrs) = adjacency.get(&node_list[0]) {
                for n in nbrs {
                    let n_idx = node_to_idx[n];
                    if n_idx != 0 {
                        candidate_set.insert(n_idx);
                    }
                }
            }

            while current_budget_spent < effective_budget && !candidate_set.is_empty() {
                let best_c = candidate_set.iter().copied().max_by(|&a, &b| {
                    let w_a = sol[x_vars[a]];
                    let w_b = sol[x_vars[b]];
                    let sc_a = node_score(&nodes[&node_list[a]], char, &options.profile);
                    let sc_b = node_score(&nodes[&node_list[b]], char, &options.profile);
                    let val_a = w_a * 10000.0 + (sc_a as f64);
                    let val_b = w_b * 10000.0 + (sc_b as f64);
                    val_a.partial_cmp(&val_b).unwrap_or(std::cmp::Ordering::Equal)
                }).unwrap();

                candidate_set.remove(&best_c);
                heuristic_selected.insert(best_c);
                if !free_set.contains(&node_list[best_c]) {
                    current_budget_spent += 1;
                }

                if let Some(nbrs) = adjacency.get(&node_list[best_c]) {
                    for n in nbrs {
                        let n_idx = node_to_idx[n];
                        if !heuristic_selected.contains(&n_idx) {
                            candidate_set.insert(n_idx);
                        }
                    }
                }
            }

            let h_selected_refs: HashSet<String> = heuristic_selected.iter().map(|&i| node_list[i].clone()).collect();
            let (h_score, h_leg_cnt, h_act_cnt, _, _) = describe_selection(&nodes, &h_selected_refs, char, &glyphs, options);
            let mut h_reward = h_score as f64;
            if is_core {
                let m1 = ((total_score_bound + 1) as f64) * m;
                let m2 = m1 * ((activation_vars.len() + 1) as f64);
                h_reward += (h_act_cnt as f64) * m1 + (h_leg_cnt as f64) * m2;
            }
            let h_obj = h_reward * m - (current_budget_spent as f64);
            if h_obj > best_obj {
                best_obj = h_obj;
                let mut h_values = vec![0.0; n_nodes];
                for &i in &heuristic_selected {
                    h_values[i] = 1.0;
                }
                best_solution_values = Some(h_values);
            }
        }

        if sol.objective() <= best_obj + 1e-6 {
            continue; // Prune
        }

        // Dominator-Aware Priority Fractional Branching:
        let mut best_frac: Option<(Variable, f64, f64)> = None; // (var, val, score)
        for pv in &decision_vars {
            let val = sol[pv.var];
            if val > 1e-4 && val < 0.9999 {
                let dist = (val - 0.5).abs();
                let score = pv.base_priority - dist * 10.0;
                if best_frac.is_none() || score > best_frac.as_ref().unwrap().2 {
                    best_frac = Some((pv.var, val, score));
                }
            }
        }

        if let Some((frac_var, frac_val, _)) = best_frac {
            let mut p0 = current_prob.clone();
            p0.add_constraint(&[(frac_var, 1.0)], ComparisonOp::Eq, 0.0);

            let mut p1 = current_prob;
            p1.add_constraint(&[(frac_var, 1.0)], ComparisonOp::Eq, 1.0);

            if frac_val >= 0.5 {
                stack.push(p0);
                stack.push(p1);
            } else {
                stack.push(p1);
                stack.push(p0);
            }
        } else {
            // Integer solution found!
            if sol.objective() > best_obj {
                best_obj = sol.objective();
                let values: Vec<f64> = x_vars.iter().map(|&v| sol[v]).collect();
                best_solution_values = Some(values);
            }
        }
    }

    if stack.is_empty() && best_solution_values.is_some() {
        is_optimal = true;
    }

    let values = best_solution_values.ok_or_else(|| {
        format!("未能在 {:.1} 秒内取得可用规划，请增加求解时间", options.time_limit)
    })?;

    let mut selected: HashSet<String> = HashSet::new();
    for (i, &val) in values.iter().enumerate() {
        if val > 0.5 {
            selected.insert(node_list[i].clone());
        }
    }

    let (ordered_steps, bfs_valid) = build_ordered_steps_bfs(&nodes, &adjacency, &root, &selected, &glyphs);
    if !bfs_valid {
        return Err("优化结果未通过连通性校验".to_string());
    }

    let mut result = build_variant_from_planned_steps(variant, &ordered_steps, available_points);
    let free_step_refs = get_free_step_refs_from_board_sequences(&result.boardSequences)?;
    if result.meta.pointCount != selected.difference(&free_step_refs).count() {
        return Err("优化结果的点数统计不一致".to_string());
    }

    // Verify board-by-board sequence reachability
    let mut executed = HashSet::new();
    for step in &result.steps {
        let s_ref = build_step_ref(step);
        if s_ref != root {
            let has_prev_neighbor = executed.iter().any(|prev: &String| {
                adjacency.get(prev).map_or(false, |nbrs| nbrs.contains(&s_ref))
            });
            if !has_prev_neighbor {
                return Err("板块顺序无法生成连通点击序列，请检查 BD 的父板和板序".to_string());
            }
        }
        executed.insert(s_ref);
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    let status_str = if is_optimal { "OPTIMAL" } else { "FEASIBLE" };

    let opt_res = make_optimization_result(
        &nodes,
        &selected,
        char,
        &glyphs,
        options,
        status_str,
        is_optimal,
        best_obj,
        upper_bound_val,
        elapsed,
        available_points,
        free_set.len(),
    );
    result.meta.strategy = Some("budget_connected_optimization".to_string());
    result.optimization = Some(serde_json::to_value(opt_res).unwrap());

    Ok(result)
}

fn build_ordered_steps_bfs(
    nodes: &HashMap<String, Step>,
    adjacency: &HashMap<String, Vec<String>>,
    root: &str,
    selected: &HashSet<String>,
    glyphs: &[super::types::GlyphModel],
) -> (Vec<Step>, bool) {
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();

    if selected.contains(root) {
        queue.push_back(root.to_string());
        seen.insert(root.to_string());
    }

    while let Some(r) = queue.pop_front() {
        let mut step = nodes[&r].clone();
        if step.glyph.is_some() {
            if let Some(g) = glyphs.iter().find(|g| g.ref_key == r) {
                if let Some(ref mut step_g) = step.glyph {
                    step_g.rank = g.rank;
                }
            }
        }
        ordered.push(step);

        if let Some(nbrs) = adjacency.get(&r) {
            for n in nbrs {
                if selected.contains(n) && !seen.contains(n) {
                    seen.insert(n.clone());
                    queue.push_back(n.clone());
                }
            }
        }
    }

    let valid = &seen == selected;
    (ordered, valid)
}
