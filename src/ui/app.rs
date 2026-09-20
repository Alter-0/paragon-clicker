use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Stroke};

use crate::automation::{
    activate_process_window, auto_detect_board_from_process, build_click_points,
    execute_board_attachment_pipeline, find_process_window, is_failsafe_triggered,
    move_mouse_and_click, open_native_grid_preview, open_native_selection_overlay,
    HotkeyEvent, HotkeyListener, PrecisionTimerGuard,
};
use crate::d2core::graph::{build_sequence_from_planner_input, build_step_ref};
use crate::d2core::model::{BoardSequence, ClickPoint, PlannerInputResult, VariantSequence};
use crate::optimizer::types::{
    get_profile_label, PlannerOptions, PROFILE_BALANCED, PROFILE_CORE, PROFILE_SURVIVAL,
};
use crate::optimizer::solver::optimize_progression;

pub enum ClickProgress {
    Progress(usize, usize, String),
    BoardSwitched(usize, (i32, i32, i32, i32)),
    Finished(bool, String),
}

pub struct ParagonClickerApp {
    // Inputs
    pub planner_url: String,
    pub target_process: String,
    pub current_points: usize,
    pub selected_strategy: String,
    pub solve_time: f64,
    pub glyph_ranks: HashMap<String, u32>,
    pub start_delay: f64,
    pub click_interval: f64,
    pub auto_connect_next_board: bool,

    // Data
    pub planner_result: Option<PlannerInputResult>,
    pub selected_variant_idx: usize,
    pub variant_sequence: Option<VariantSequence>,
    pub selected_board_idx: usize,
    pub selected_region: Option<(i32, i32, i32, i32)>,
    pub click_points: Vec<ClickPoint>,
    pub logs: Vec<String>,

    // Background workers
    pub is_resolving: bool,
    pub is_planning: bool,
    pub is_clicking: bool,
    pub resolve_rx: Option<Receiver<Result<PlannerInputResult, String>>>,
    pub strategy_rx: Option<Receiver<Result<VariantSequence, String>>>,
    pub click_rx: Option<Receiver<ClickProgress>>,
    pub selection_rx: Option<Receiver<Option<(i32, i32, i32, i32)>>>,
    pub hotkey_rx: Option<Receiver<HotkeyEvent>>,
    pub _hotkey_listener: Option<HotkeyListener>,
    pub click_cancel_flag: Arc<AtomicBool>,
    pub font_initialized: bool,
    pub start_clicking_after_planning: bool,
    pub last_optimized_params: Option<(usize, usize, String, HashMap<String, u32>)>,
}

pub fn setup_custom_fonts(ctx: &egui::Context) {
    let candidate_paths = [
        "C:\\Windows\\Fonts\\msyh.ttc",   // 微软雅黑 (Microsoft YaHei)
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simhei.ttf", // 黑体 (SimHei)
        "C:\\Windows\\Fonts\\simsun.ttc", // 宋体 (SimSun)
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = false;

    for path in candidate_paths {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk_font".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk_font".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk_font".to_owned());
            loaded = true;
            break;
        }
    }

    if loaded {
        ctx.set_fonts(fonts);
    }
}

impl Default for ParagonClickerApp {
    fn default() -> Self {
        let (hotkey_tx, hotkey_rx) = channel();
        let hotkey_listener = HotkeyListener::new(hotkey_tx);

        Self {
            planner_url: "https://www.d2core.com/d4/planner?bd=1Tok".to_string(),
            target_process: "Diablo IV.exe".to_string(),
            current_points: 0,
            selected_strategy: PROFILE_BALANCED.to_string(),
            solve_time: 5.0,
            glyph_ranks: HashMap::new(),
            start_delay: 3.0,
            click_interval: 0.12,
            auto_connect_next_board: true,

            planner_result: None,
            selected_variant_idx: 0,
            variant_sequence: None,
            selected_board_idx: 0,
            selected_region: None,
            click_points: Vec::new(),
            logs: vec![
                "巅峰加点器 (Rust 极致性能版) 已就绪。".to_string(),
                "已启动全局热键监听：[F8] 一键识别并加点 | [F7] 单独识别盘面 | [F9] 停止加点。".to_string(),
            ],

            is_resolving: false,
            is_planning: false,
            is_clicking: false,
            resolve_rx: None,
            strategy_rx: None,
            click_rx: None,
            selection_rx: None,
            hotkey_rx: Some(hotkey_rx),
            _hotkey_listener: Some(hotkey_listener),
            click_cancel_flag: Arc::new(AtomicBool::new(false)),
            font_initialized: false,
            start_clicking_after_planning: false,
            last_optimized_params: None,
        }
    }
}

impl ParagonClickerApp {
    pub fn log(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        self.logs.push(text);
        if self.logs.len() > 300 {
            self.logs.remove(0);
        }
    }

    pub fn current_board(&self) -> Option<&BoardSequence> {
        let var = self.variant_sequence.as_ref()?;
        var.boardSequences.get(self.selected_board_idx)
    }

    pub fn refresh_click_points(&mut self) {
        if let (Some(board), Some((l, t, r, b))) = (self.current_board(), self.selected_region) {
            self.click_points = build_click_points(board, l, t, r, b);
        } else {
            self.click_points.clear();
        }
    }

    pub fn auto_detect_region(&mut self) {
        self.log(format!("正在捕获「{}」游戏画面并进行红框投影智能定位...", self.target_process));
        match auto_detect_board_from_process(&self.target_process) {
            Ok((l, t, r, b)) => {
                self.selected_region = Some((l, t, r, b));
                self.refresh_click_points();
                self.log(format!(
                    "🎯 自动识别成功！定位到 21×21 盘面：({l}, {t}) -> ({r}, {b}) [尺寸: {}x{}]",
                    r - l, b - t
                ));
            }
            Err(e) => {
                self.log(format!("⚠️ 自动识别失败：{e}"));
            }
        }
    }

    pub fn start_resolve_url(&mut self) {
        let url = self.planner_url.trim().to_string();
        if url.is_empty() {
            self.log("解析失败：请输入有效的规划器链接。");
            return;
        }

        self.is_resolving = true;
        self.log(format!("正在异步获取并解析暗黑核构筑：{url} ..."));

        let (tx, rx): (Sender<Result<PlannerInputResult, String>>, Receiver<Result<PlannerInputResult, String>>) = channel();
        self.resolve_rx = Some(rx);

        thread::spawn(move || {
            let res = build_sequence_from_planner_input(&url);
            let _ = tx.send(res);
        });
    }

    pub fn start_apply_strategy(&mut self) {
        let Some(variant_clone) = self
            .planner_result
            .as_ref()
            .and_then(|r| r.variants.get(self.selected_variant_idx).cloned())
        else {
            return;
        };

        self.is_planning = true;
        self.log(format!(
            "正在计算加点优化策略（目标点数：{}，时限：{:.1}s）...",
            self.current_points, self.solve_time
        ));

        let budget = self.current_points;
        let options = PlannerOptions {
            profile: self.selected_strategy.clone(),
            glyph_rank: None,
            glyph_ranks: self.glyph_ranks.clone(),
            time_limit: self.solve_time,
        };

        let (tx, rx): (Sender<Result<VariantSequence, String>>, Receiver<Result<VariantSequence, String>>) = channel();
        self.strategy_rx = Some(rx);

        thread::spawn(move || {
            let res = optimize_progression(&variant_clone, budget, &options);
            let _ = tx.send(res);
        });
    }

    /// One-click automated entry: automatically brings game to front, detects board if not yet done,
    /// refreshes click points, and immediately starts clicking pipeline.
    pub fn start_or_auto_detect_and_start_clicking(&mut self) {
        if self.is_clicking {
            return;
        }

        if self.current_board().is_none() {
            self.log("请先解析暗黑核链接获取巅峰盘数据！");
            return;
        }

        // If solver is currently running, queue clicking to begin right after it finishes
        if self.is_planning {
            self.log("正在计算最优加点路径中，计算完成后将立即全自动开始加点...");
            self.start_clicking_after_planning = true;
            return;
        }

        // Check if user specified fewer points than BD full points
        let bd_points = self
            .planner_result
            .as_ref()
            .and_then(|r| r.variants.get(self.selected_variant_idx))
            .map(|v| v.meta.fullPointCount.unwrap_or(v.meta.pointCount))
            .unwrap_or(0);

        let is_already_optimized = self.last_optimized_params.as_ref()
            == Some(&(
                self.selected_variant_idx,
                self.current_points,
                self.selected_strategy.clone(),
                self.glyph_ranks.clone(),
            ));

        if self.current_points < bd_points && !is_already_optimized {
            self.log(format!(
                "⚡ 当前可用巅峰点 ({} 点) 小于 BD 设定点数 ({} 点)，正在全自动求解最优路径...",
                self.current_points, bd_points
            ));
            self.start_clicking_after_planning = true;
            self.start_apply_strategy();
            return;
        }

        // Auto-detect region if not yet identified
        if self.selected_region.is_none() {
            self.log("🚀 一键开始加点：未检测到盘面区域，正在自动激活游戏并定位...");
            let target_proc = self.target_process.trim().to_string();
            if !target_proc.is_empty() {
                let _ = activate_process_window(&target_proc);
                thread::sleep(Duration::from_millis(150));
            }
            self.auto_detect_region();
            if self.selected_region.is_none() {
                self.log("⚠️ 一键加点已中断：未能自动识别到巅峰盘，请确认游戏窗口处于前台且已打开巅峰盘页面。");
                return;
            }
        }

        self.refresh_click_points();
        self.start_clicking();
    }

    pub fn start_clicking(&mut self) {
        // If current points < BD points and not yet optimized, divert to start_or_auto_detect_and_start_clicking
        let bd_points = self
            .planner_result
            .as_ref()
            .and_then(|r| r.variants.get(self.selected_variant_idx))
            .map(|v| v.meta.fullPointCount.unwrap_or(v.meta.pointCount))
            .unwrap_or(0);

        let is_already_optimized = self.last_optimized_params.as_ref()
            == Some(&(
                self.selected_variant_idx,
                self.current_points,
                self.selected_strategy.clone(),
                self.glyph_ranks.clone(),
            ));

        if (self.current_points < bd_points && !is_already_optimized) || self.is_planning {
            self.start_or_auto_detect_and_start_clicking();
            return;
        }

        // If region not yet set, attempt auto detection
        if self.selected_region.is_none() {
            let target_proc = self.target_process.trim().to_string();
            if !target_proc.is_empty() {
                let _ = activate_process_window(&target_proc);
                thread::sleep(Duration::from_millis(150));
            }
            self.auto_detect_region();
        }

        if self.click_points.is_empty() {
            self.refresh_click_points();
        }

        if self.click_points.is_empty() {
            self.log("未生成点击点，请确认已解析构筑并选择板块。");
            return;
        }

        let target_proc = self.target_process.trim().to_string();
        if target_proc.is_empty() {
            self.log("请输入目标进程名。");
            return;
        }

        let all_boards = self.variant_sequence.as_ref().map(|v| v.boardSequences.clone()).unwrap_or_default();
        let start_board_idx = self.selected_board_idx;
        let auto_connect = self.auto_connect_next_board;
        let initial_region = match self.selected_region {
            Some(reg) => reg,
            None => {
                self.log("未确定盘面区域，请按 F7 自动识别或手动框选。");
                return;
            }
        };

        let delay = self.start_delay;
        let interval = self.click_interval;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.click_cancel_flag = Arc::clone(&cancel_flag);
        self.is_clicking = true;

        self.log(format!(
            "准备开始执行加点。当前从第 {}/{} 盘开始，跨盘全自动: {}。先激活进程 {}...",
            start_board_idx + 1,
            all_boards.len(),
            if auto_connect { "开启" } else { "关闭" },
            target_proc
        ));

        let (tx, rx): (Sender<ClickProgress>, Receiver<ClickProgress>) = channel();
        self.click_rx = Some(rx);

        thread::spawn(move || {
            let _timer_guard = PrecisionTimerGuard::new();

            // Start delay
            if delay > 0.0 {
                let end_at = Instant::now() + Duration::from_secs_f64(delay);
                while Instant::now() < end_at {
                    if cancel_flag.load(Ordering::Relaxed) {
                        let _ = tx.send(ClickProgress::Finished(false, "点击前已取消".to_string()));
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }

            let _ = tx.send(ClickProgress::Progress(
                0,
                0,
                format!("正在激活目标进程窗口：{target_proc}"),
            ));

            if !activate_process_window(&target_proc) {
                let _ = tx.send(ClickProgress::Finished(
                    false,
                    format!("未找到进程 {target_proc} 的可见窗口"),
                ));
                return;
            }

            let mut curr_board_idx = start_board_idx;
            let mut curr_region = initial_region;

            while curr_board_idx < all_boards.len() {
                let curr_board = &all_boards[curr_board_idx];
                let (l, t, r, b) = curr_region;
                let points = build_click_points(curr_board, l, t, r, b);

                let _ = tx.send(ClickProgress::Progress(
                    0,
                    points.len(),
                    format!("开始加点：第 {}/{} 盘【{}】(共 {} 个节点)", curr_board_idx + 1, all_boards.len(), curr_board.boardName, points.len()),
                ));

                for (idx, pt) in points.iter().enumerate() {
                    if cancel_flag.load(Ordering::Relaxed) {
                        let _ = tx.send(ClickProgress::Finished(false, "用户已停止".to_string()));
                        return;
                    }
                    if is_failsafe_triggered() {
                        let _ = tx.send(ClickProgress::Finished(false, "已触发安全停止 (鼠标移动到左上角)".to_string()));
                        return;
                    }

                    if let Err(e) = move_mouse_and_click(pt.x, pt.y) {
                        let _ = tx.send(ClickProgress::Finished(false, format!("点击执行失败：{e}")));
                        return;
                    }

                    let _ = tx.send(ClickProgress::Progress(
                        idx + 1,
                        points.len(),
                        format!("[第{}/{}盘] {}. {} -> ({}, {})", curr_board_idx + 1, all_boards.len(), pt.local_step, pt.node_name, pt.x, pt.y),
                    ));

                    if idx + 1 < points.len() && interval > 0.0 {
                        thread::sleep(Duration::from_secs_f64(interval));
                    }
                }

                // Check if there is a next board to connect
                if !auto_connect || curr_board_idx + 1 >= all_boards.len() {
                    break;
                }

                let next_board = &all_boards[curr_board_idx + 1];
                let _ = tx.send(ClickProgress::Progress(
                    points.len(),
                    points.len(),
                    format!("盘面【{}】点击完毕，正在自动触发出口门并附接【{}】(旋转 {} 次)...", curr_board.boardName, next_board.boardName, next_board.boardRotate),
                ));

                let hwnd = match find_process_window(&target_proc) {
                    Some(h) => h,
                    None => {
                        let _ = tx.send(ClickProgress::Finished(false, format!("未找到进程 {target_proc} 窗口")));
                        return;
                    }
                };

                let cancel_check = || cancel_flag.load(Ordering::Relaxed);
                let bw = (r - l).max(1) as u32;
                let bh = (b - t).max(1) as u32;
                let current_rect = (l, t, bw, bh);

                match execute_board_attachment_pipeline(hwnd, current_rect, curr_board, next_board, &target_proc, &cancel_check) {
                    Ok((nl, nt, nr, nb)) => {
                        curr_board_idx += 1;
                        curr_region = (nl, nt, nr, nb);
                        let _ = tx.send(ClickProgress::BoardSwitched(curr_board_idx, (nl, nt, nr, nb)));
                        thread::sleep(Duration::from_millis(500));
                    }
                    Err(e) => {
                        let _ = tx.send(ClickProgress::Finished(false, format!("跨盘连接中断：{e}")));
                        return;
                    }
                }
            }

            let _ = tx.send(ClickProgress::Finished(true, "全流程加点与跨盘附接已顺利完成！".to_string()));
        });
    }

    pub fn stop_clicking(&mut self) {
        self.start_clicking_after_planning = false;
        if self.is_clicking {
            self.click_cancel_flag.store(true, Ordering::Relaxed);
            self.log("已发送停止信号...");
        }
    }

    pub fn poll_channels(&mut self) {
        if let Some(ref rx) = self.resolve_rx {
            if let Ok(res) = rx.try_recv() {
                self.is_resolving = false;
                self.resolve_rx = None;
                match res {
                    Ok(parsed) => {
                        let title = parsed.meta.title.clone().unwrap_or_else(|| "未知".to_string());
                        let var_count = parsed.variants.len();
                        self.log(format!("构筑解析成功：{title}，共 {var_count} 个变体。"));

                        self.selected_variant_idx = parsed.meta.selectedVariantIndex;
                        let max_points = parsed
                            .variants
                            .iter()
                            .map(|v| v.meta.pointCount)
                            .max()
                            .unwrap_or(0);
                        self.current_points = max_points;
                        self.planner_result = Some(parsed);

                        self.init_variant();
                    }
                    Err(e) => {
                        self.log(format!("解析失败：{e}"));
                    }
                }
            }
        }

        if let Some(ref rx) = self.strategy_rx {
            if let Ok(res) = rx.try_recv() {
                self.is_planning = false;
                self.strategy_rx = None;
                match res {
                    Ok(planned) => {
                        let score = planned
                            .optimization
                            .as_ref()
                            .and_then(|o| o.get("score"))
                            .and_then(|s| s.as_i64())
                            .unwrap_or(0);
                        let seconds = planned
                            .optimization
                            .as_ref()
                            .and_then(|o| o.get("seconds"))
                            .and_then(|s| s.as_f64())
                            .unwrap_or(0.0);
                        let status = planned
                            .optimization
                            .as_ref()
                            .and_then(|o| o.get("status"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("-");

                        self.log(format!(
                            "加点规划完成！状态：{}，评分：{}，耗时：{:.3}s (极速)",
                            status, score, seconds
                        ));

                        self.last_optimized_params = Some((
                            self.selected_variant_idx,
                            self.current_points,
                            self.selected_strategy.clone(),
                            self.glyph_ranks.clone(),
                        ));

                        self.variant_sequence = Some(planned);
                        self.selected_board_idx = 0;
                        self.refresh_click_points();

                        if self.start_clicking_after_planning {
                            self.start_clicking_after_planning = false;
                            self.log("🚀 最优加点路径已就绪，立即全自动开始加点！");
                            self.start_or_auto_detect_and_start_clicking();
                        }
                    }
                    Err(e) => {
                        self.start_clicking_after_planning = false;
                        self.log(format!("策略计算失败：{e}"));
                    }
                }
            }
        }

        let mut click_msgs = Vec::new();
        if let Some(ref rx) = self.click_rx {
            while let Ok(msg) = rx.try_recv() {
                click_msgs.push(msg);
            }
        }
        for msg in click_msgs {
            match msg {
                ClickProgress::Progress(curr, total, text) => {
                    self.log(format!("[{curr}/{total}] {text}"));
                }
                ClickProgress::BoardSwitched(new_idx, (l, t, r, b)) => {
                    self.selected_board_idx = new_idx;
                    self.selected_region = Some((l, t, r, b));
                    self.refresh_click_points();
                    if let Some(b) = self.current_board() {
                        self.log(format!("🔄 成功自动连接并附接下一盘：【{}】(旋转 {} 次)，立即开始该盘加点！", b.boardName, b.boardRotate));
                    }
                }
                ClickProgress::Finished(ok, text) => {
                    self.is_clicking = false;
                    self.log(format!("执行结束：{text} (成功={ok})"));
                }
            }
        }

        if let Some(ref rx) = self.selection_rx {
            if let Ok(res) = rx.try_recv() {
                if let Some((l, t, r, b)) = res {
                    self.selected_region = Some((l, t, r, b));
                    self.refresh_click_points();
                    self.log(format!("选区成功：({l}, {t}) -> ({r}, {b}) [宽={}, 高={}]", r - l, b - t));
                } else {
                    self.log("已取消区域选择。");
                }
                self.selection_rx = None;
            }
        }

        let mut hotkey_events = Vec::new();
        if let Some(ref rx) = self.hotkey_rx {
            while let Ok(evt) = rx.try_recv() {
                hotkey_events.push(evt);
            }
        }
        for evt in hotkey_events {
            match evt {
                HotkeyEvent::Detect => {
                    self.log("【全局热键 F7】触发：正在自动识别游戏巅峰盘区域...");
                    self.auto_detect_region();
                }
                HotkeyEvent::Start => {
                    if !self.is_clicking {
                        self.log("【全局热键 F8】触发：一键开始加点...");
                        self.start_or_auto_detect_and_start_clicking();
                    }
                }
                HotkeyEvent::Stop => {
                    if self.is_clicking {
                        self.log("【全局热键 F9】触发：紧急停止点击！");
                        self.stop_clicking();
                    }
                }
            }
        }
    }

    pub fn init_variant(&mut self) {
        let mut ranks = HashMap::new();
        if let Some(ref full) = self.planner_result {
            if let Some(var) = full.variants.get(self.selected_variant_idx) {
                for step in &var.globalSteps {
                    if let Some(ref glyph) = step.glyph {
                        ranks.insert(build_step_ref(step), glyph.rank);
                    }
                }
            }
        }
        self.glyph_ranks = ranks;
        self.last_optimized_params = None;
        self.start_apply_strategy();
    }
}

impl eframe::App for ParagonClickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.font_initialized {
            setup_custom_fonts(ctx);
            self.font_initialized = true;
        }

        self.poll_channels();

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.render_main_ui(ui);
            });
        });

        // Request repaint while working, awaiting selection, or polling hotkeys
        if self.is_resolving || self.is_planning || self.is_clicking || self.selection_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(30));
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl ParagonClickerApp {
    fn render_main_ui(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

        // Header with Live Status
        ui.horizontal(|ui| {
            ui.heading("⚡ 巅峰加点器 (Paragon Clicker Rust - 极致性能版)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.is_resolving {
                    ui.spinner();
                    ui.colored_label(Color32::from_rgb(100, 180, 255), "正在网络请求 BD 数据...");
                } else if self.is_planning {
                    ui.spinner();
                    ui.colored_label(Color32::from_rgb(255, 180, 80), "正在运行纯 Rust MIP 求解器...");
                } else if self.is_clicking {
                    ui.spinner();
                    ui.colored_label(Color32::from_rgb(255, 90, 90), "正在自动加点中 (鼠标移至屏幕最左上角即刻安全中止)...");
                } else {
                    ui.colored_label(Color32::from_rgb(80, 220, 120), "● 系统就绪");
                }
            });
        });
        ui.add_space(2.0);

        // Guidance Banner
        egui::Frame::none()
            .fill(Color32::from_rgb(32, 40, 56))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 80, 110)))
            .rounding(4.0)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("💡");
                    ui.colored_label(
                        Color32::from_rgb(200, 220, 255),
                        "操作引导：1. 输入暗黑核配装地址并点击「解析链接」；2. 设定当前可用巅峰点（小于BD全满将自动计算最优解）；3. 点击「▶ 🚀 一键开始加点 (F8)」即可全自动识别并加点。",
                    );
                });
            });

        ui.add_space(4.0);

        // Group 1: D2Core 链接与策略配置
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("D2Core 规划器链接与优化配置");
            ui.horizontal(|ui| {
                ui.label("规划器链接：");
                let text_w = (ui.available_width() - 110.0).max(200.0);
                ui.add(egui::TextEdit::singleline(&mut self.planner_url).desired_width(text_w));
                if ui
                    .add_enabled(!self.is_resolving && !self.is_planning, egui::Button::new("🔍 解析链接"))
                    .clicked()
                {
                    self.start_resolve_url();
                }
            });

            let var_labels: Vec<String> = self.planner_result.as_ref().map(|p| {
                p.variants.iter().enumerate().map(|(idx, v)| {
                    let name = v.meta.variantName.as_deref().unwrap_or("未命名");
                    format!("{idx}: {name}")
                }).collect()
            }).unwrap_or_default();

            if !var_labels.is_empty() {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("变体：");
                    let current_text = var_labels.get(self.selected_variant_idx).cloned().unwrap_or_default();
                    egui::ComboBox::from_id_source("variant_combo")
                        .selected_text(current_text)
                        .show_ui(ui, |ui| {
                            for (idx, label) in var_labels.iter().enumerate() {
                                if ui.selectable_value(&mut self.selected_variant_idx, idx, label).clicked() {
                                    changed = true;
                                }
                            }
                        });

                    if let Some(ref var) = self.variant_sequence {
                        ui.label(format!("| 职业：{}", var.meta.char.as_deref().unwrap_or("-")));
                        ui.label(format!("| 全满总点数：{}", var.meta.fullPointCount.unwrap_or(var.meta.pointCount)));
                    }
                });
                if changed {
                    self.init_variant();
                }
            }

            ui.horizontal(|ui| {
                ui.label("目标进程：");
                ui.add(egui::TextEdit::singleline(&mut self.target_process).desired_width(160.0));

                ui.label("当前可用巅峰点：");
                let max_p = self.planner_result.as_ref()
                    .and_then(|r| r.variants.get(self.selected_variant_idx))
                    .map(|v| v.meta.fullPointCount.unwrap_or(v.meta.pointCount))
                    .unwrap_or(400);
                ui.add(egui::DragValue::new(&mut self.current_points).range(0..=max_p));
                ui.colored_label(
                    Color32::from_rgb(180, 200, 220),
                    format!("(BD全满: {max_p} 点，小于全满时一键加点将自动计算最优解)"),
                );
            });

            ui.horizontal(|ui| {
                ui.label("优化策略：");
                egui::ComboBox::from_id_source("strategy_combo")
                    .selected_text(get_profile_label(&self.selected_strategy))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.selected_strategy, PROFILE_BALANCED.to_string(), get_profile_label(PROFILE_BALANCED));
                        ui.selectable_value(&mut self.selected_strategy, PROFILE_CORE.to_string(), get_profile_label(PROFILE_CORE));
                        ui.selectable_value(&mut self.selected_strategy, PROFILE_SURVIVAL.to_string(), get_profile_label(PROFILE_SURVIVAL));
                    });

                ui.label("求解时限：");
                ui.add(egui::DragValue::new(&mut self.solve_time).range(1.0..=60.0).suffix(" 秒"));
            });

            // Glyph Ranks
            if !self.glyph_ranks.is_empty() {
                ui.collapsing("实际雕文等级配置（0=未装备，修改后一键加点将自动优化）：", |ui| {
                    let mut keys: Vec<String> = self.glyph_ranks.keys().cloned().collect();
                    keys.sort();
                    ui.horizontal_wrapped(|ui| {
                        for k in keys {
                            let rank = self.glyph_ranks.get_mut(&k).unwrap();
                            ui.label(format!("{k}:"));
                            ui.add(egui::DragValue::new(rank).range(0..=200));
                        }
                    });
                });
            }
        });

        ui.add_space(4.0);

        // Group 2: 板块选择与点击设置
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("板块设置与自动点击控制");
            ui.horizontal(|ui| {
                ui.label("选择板块：");
                let board_labels: Vec<String> = self.variant_sequence.as_ref().map(|v| {
                    v.boardSequences.iter().map(|b| b.label()).collect()
                }).unwrap_or_default();

                if !board_labels.is_empty() {
                    let mut board_changed = false;
                    let current_text = board_labels.get(self.selected_board_idx).cloned().unwrap_or_default();
                    egui::ComboBox::from_id_source("board_combo")
                        .selected_text(current_text)
                        .show_ui(ui, |ui| {
                            for (idx, label) in board_labels.iter().enumerate() {
                                if ui.selectable_value(&mut self.selected_board_idx, idx, label).clicked() {
                                    board_changed = true;
                                }
                            }
                        });
                    if board_changed {
                        self.refresh_click_points();
                    }
                } else {
                    ui.label("(请先解析链接)");
                }

                ui.label("开始延迟：");
                ui.add(egui::DragValue::new(&mut self.start_delay).range(0.0..=30.0).suffix(" 秒"));

                ui.label("点击间隔：");
                ui.add(egui::DragValue::new(&mut self.click_interval).range(0.01..=5.0).suffix(" 秒"));
            });

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.auto_connect_next_board,
                    "🔗 跨盘全自动流水线：当前盘点完后自动点击出口门、选盘、旋转并附接下一盘，连续加点",
                );
            });

            ui.horizontal(|ui| {
                let has_board = self.current_board().is_some();
                let has_region = self.selected_region.is_some();

                // 1. One-Click Start (Primary Action, F8)
                if ui
                    .add_enabled(!self.is_clicking && has_board, egui::Button::new("▶ 🚀 一键开始加点 (F8)"))
                    .clicked()
                {
                    self.start_or_auto_detect_and_start_clicking();
                }

                // 2. Individual helper actions
                if ui.add_enabled(has_board && !self.is_clicking, egui::Button::new("🎯 自动识别盘面 (F7)")).clicked() {
                    self.auto_detect_region();
                }

                if ui.add_enabled(has_board && !self.is_clicking, egui::Button::new("📐 手动框选")).clicked() {
                    let preview_cells = self.current_board().map(|b| {
                        b.steps.iter().map(|s| (s.rotatedCoord.row, s.rotatedCoord.col)).collect()
                    }).unwrap_or_default();
                    let (tx, rx) = channel();
                    self.selection_rx = Some(rx);
                    self.log("【选区模式】已打开全屏半透明选区窗口。请在游戏画面中按住鼠标左键从左上角拖拽到右下角，按 Esc 取消。");
                    open_native_selection_overlay(preview_cells, tx);
                }

                if ui.add_enabled(has_board && has_region && !self.is_clicking, egui::Button::new("👁️ 预览 21×21 网格")).clicked() {
                    if let Some(reg) = self.selected_region {
                        let points: Vec<(i32, i32)> = self.click_points.iter().map(|p| (p.x, p.y)).collect();
                        self.log("【网格预览】已打开全屏网格预览。单击鼠标任意处或按 Esc 键关闭。");
                        open_native_grid_preview(reg, points);
                    }
                }

                if ui.add_enabled(self.is_clicking, egui::Button::new("⏹ 停止点击 (F9)")).clicked() {
                    self.stop_clicking();
                }
            });

            ui.horizontal(|ui| {
                if let Some((l, t, r, b)) = self.selected_region {
                    ui.colored_label(
                        Color32::from_rgb(100, 220, 120),
                        format!("● 当前已选定区域：Left={l}, Top={t}, Right={r}, Bottom={b} (宽={}, 高={})", r - l, b - t)
                    );
                } else {
                    ui.colored_label(
                        Color32::from_rgb(255, 180, 80),
                        "● 当前区域：尚未定位（可直接点击「▶ 🚀 一键开始加点 (F8)」，程序将全自动识别定位并加点）。"
                    );
                }
            });

            ui.colored_label(
                Color32::from_rgb(170, 190, 210),
                "⌨️ 全局热键：[F8] 一键自动识别并加点 | [F7] 单独识别盘面 | [F9] 紧急停止 | 亦可甩鼠标至屏幕最左上角 (0, 0) 防失控安全退出"
            );
        });

        ui.add_space(4.0);

        // Group 3: 详细信息与点击表格 (双栏水平自适应)
        let available_width = ui.available_width();
        let left_width = (available_width * 0.40).max(360.0);
        let right_width = (available_width - left_width - 16.0).max(320.0);

        ui.horizontal(|ui| {
            // Left column: Info panel
            ui.allocate_ui(egui::vec2(left_width, 260.0), |ui| {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.heading("规划详情与状态");
                    egui::ScrollArea::vertical().max_height(230.0).show(ui, |ui| {
                        if let Some(ref var) = self.variant_sequence {
                            ui.label(format!("职业：{}", var.meta.char.as_deref().unwrap_or("-")));
                            ui.label(format!("变体：{}", var.meta.variantName.as_deref().unwrap_or("-")));
                            ui.label(format!("总板块数：{}", var.meta.boardCount));
                            ui.label(format!("点数预算：{} / 全满 {}", var.meta.pointCount, var.meta.fullPointCount.unwrap_or(var.meta.pointCount)));

                            if let Some(ref opt) = var.optimization {
                                ui.separator();
                                let status = opt.get("status").and_then(|s| s.as_str()).unwrap_or("-");
                                let score = opt.get("score").and_then(|s| s.as_i64()).unwrap_or(0);
                                let leg = opt.get("legendaryCount").and_then(|s| s.as_u64()).unwrap_or(0);
                                let act = opt.get("activeGlyphCount").and_then(|s| s.as_u64()).unwrap_or(0);
                                let hp = opt.get("lifePercent").and_then(|s| s.as_f64()).unwrap_or(0.0);
                                let sec = opt.get("seconds").and_then(|s| s.as_f64()).unwrap_or(0.0);

                                ui.label(format!("求解状态：{status} (耗时: {sec:.3}s)"));
                                ui.label(format!("模型综合评分：{score}"));
                                ui.label(format!("包含传奇节点：{leg} 个"));
                                ui.label(format!("已激活雕文：{act} 个"));
                                ui.label(format!("基础生命加成：{hp:.1}%"));

                                if let Some(glyphs) = opt.get("glyphs").and_then(|g| g.as_array()) {
                                    ui.separator();
                                    ui.label("雕文激活详情：");
                                    for g in glyphs {
                                        let name = g.get("name").and_then(|s| s.as_str()).unwrap_or("-");
                                        let rank = g.get("rank").and_then(|s| s.as_u64()).unwrap_or(0);
                                        let act = g.get("active").and_then(|s| s.as_bool()).unwrap_or(false);
                                        let act_str = if act { "✅ 已激活" } else { "❌ 未激活" };
                                        ui.label(format!(" • {name} (Lv.{rank}): {act_str}"));
                                    }
                                }
                            }
                        } else {
                            ui.label("暂无规划数据，请输入链接并点击「解析链接」。");
                        }
                    });
                });
            });

            // Right column: Steps Table
            ui.allocate_ui(egui::vec2(right_width, 260.0), |ui| {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.heading(format!("板块点击节点清单 (当前板块共 {} 步)", self.click_points.len()));
                    egui::ScrollArea::vertical().max_height(230.0).show(ui, |ui| {
                        egui::Grid::new("click_points_grid")
                            .striped(true)
                            .min_col_width(45.0)
                            .show(ui, |ui| {
                                ui.strong("步骤");
                                ui.strong("板内");
                                ui.strong("节点名称");
                                ui.strong("类型");
                                ui.strong("网格(行,列)");
                                ui.strong("屏幕像素(X, Y)");
                                ui.end_row();

                                if self.click_points.is_empty() {
                                    ui.label("-");
                                    ui.label("-");
                                    ui.label("请先选择区域以计算坐标");
                                    ui.label("-");
                                    ui.label("-");
                                    ui.label("-");
                                    ui.end_row();
                                }

                                for pt in &self.click_points {
                                    ui.label(pt.step.to_string());
                                    ui.label(pt.local_step.to_string());
                                    ui.label(&pt.node_name);
                                    ui.label(&pt.node_kind);
                                    ui.label(format!("({}, {})", pt.row, pt.col));
                                    ui.label(format!("({}, {})", pt.x, pt.y));
                                    ui.end_row();
                                }
                            });
                    });
                });
            });
        });

        ui.add_space(4.0);

        // Group 4: 运行日志
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("运行日志");
            egui::ScrollArea::vertical().max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
                for line in &self.logs {
                    ui.label(line);
                }
            });
        });
    }
}

