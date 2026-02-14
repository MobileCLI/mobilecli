//! CLI detection + waiting-state classification
//!
//! This module provides:
//! - A scored CLI identity tracker
//! - ANSI-stripped prompt detection
//! - Normalized waiting-state classification

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use strip_ansi_escapes::strip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CliType {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    Terminal,
    Unknown,
}

impl CliType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CliType::Claude => "claude",
            CliType::Codex => "codex",
            CliType::Gemini => "gemini",
            CliType::OpenCode => "opencode",
            CliType::Terminal => "terminal",
            CliType::Unknown => "unknown",
        }
    }

    pub fn default_approval_model(&self) -> ApprovalModel {
        match self {
            CliType::Claude => ApprovalModel::Numbered,
            CliType::Codex => ApprovalModel::Numbered,
            CliType::Gemini => ApprovalModel::YesNo,
            CliType::OpenCode => ApprovalModel::Arrow,
            CliType::Terminal | CliType::Unknown => ApprovalModel::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalModel {
    Numbered,
    YesNo,
    Arrow,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitType {
    ToolApproval,
    PlanApproval,
    ClarifyingQuestion,
    AwaitingResponse,
}

impl WaitType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WaitType::ToolApproval => "tool_approval",
            WaitType::PlanApproval => "plan_approval",
            WaitType::ClarifyingQuestion => "clarifying_question",
            WaitType::AwaitingResponse => "awaiting_response",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WaitEvent {
    pub wait_type: WaitType,
    pub prompt: String,
    pub approval_model: ApprovalModel,
    pub prompt_hash: u64,
}

#[derive(Debug, Clone)]
pub struct CliTracker {
    scores: HashMap<CliType, i32>,
    current: CliType,
    confidence: u8,
    last_updated: DateTime<Utc>,
}

impl CliTracker {
    pub fn new() -> Self {
        let mut scores = HashMap::new();
        scores.insert(CliType::Terminal, 1);
        scores.insert(CliType::Unknown, 0);
        scores.insert(CliType::Claude, 0);
        scores.insert(CliType::Codex, 0);
        scores.insert(CliType::Gemini, 0);
        scores.insert(CliType::OpenCode, 0);
        Self {
            scores,
            current: CliType::Terminal,
            confidence: 1,
            last_updated: Utc::now(),
        }
    }

    pub fn current(&self) -> CliType {
        self.current
    }

    pub fn apply_signal(&mut self, cli: CliType, weight: i32) {
        let entry = self.scores.entry(cli).or_insert(0);
        *entry += weight;
        self.last_updated = Utc::now();
        self.select_best();
    }

    pub fn update_from_command(&mut self, command: &str) {
        if let Some(cli) = cli_from_command(command) {
            // Strong signal: command name
            self.apply_signal(cli, 8);
        }
    }

    pub fn update_from_output(&mut self, text: &str) {
        if let Some(cli) = cli_from_output(text) {
            // Moderate signal: banner/output patterns
            self.apply_signal(cli, 4);
        }
    }

    fn select_best(&mut self) {
        let mut best = self.current;
        let mut best_score = *self.scores.get(&best).unwrap_or(&0);

        for (cli, score) in &self.scores {
            if *score > best_score {
                best = *cli;
                best_score = *score;
            }
        }

        let current_score = *self.scores.get(&self.current).unwrap_or(&0);
        // Hysteresis: require a margin + threshold to switch
        if best_score >= 5 && best_score >= current_score + 2 {
            self.current = best;
        }

        // Confidence: coarse bucket of the best score
        self.confidence = (best_score / 3).clamp(0, 3) as u8;
    }
}

fn cli_from_command(command: &str) -> Option<CliType> {
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
        .to_lowercase();

    match base.as_str() {
        "claude" | "claude-code" | "claude_code" => Some(CliType::Claude),
        "codex" => Some(CliType::Codex),
        "gemini" | "gemini-cli" | "gemini_cli" => Some(CliType::Gemini),
        "opencode" | "open-code" | "open_code" => Some(CliType::OpenCode),
        _ => None,
    }
}

fn cli_from_output(text: &str) -> Option<CliType> {
    let lower = text.to_lowercase();

    // Conservative banner detection (avoid false positives in conversations)
    if lower.contains("claude code") || (lower.contains("anthropic") && lower.contains("claude")) {
        return Some(CliType::Claude);
    }
    if lower.contains("openai codex") || (lower.contains("codex") && lower.contains("openai")) {
        return Some(CliType::Codex);
    }
    if lower.contains("gemini cli") || (lower.contains("gemini") && lower.contains("google")) {
        return Some(CliType::Gemini);
    }
    if lower.contains("opencode") || lower.contains("open code") {
        return Some(CliType::OpenCode);
    }

    None
}

pub fn strip_ansi_and_normalize(input: &str) -> String {
    let stripped = strip(input.as_bytes());
    String::from_utf8_lossy(&stripped).to_string()
}

fn tail_chars(input: &str, max_chars: usize) -> String {
    let len = input.chars().count();
    if len <= max_chars {
        return input.to_string();
    }
    input.chars().skip(len - max_chars).collect()
}

fn hash_prompt(prompt: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut hasher);
    hasher.finish()
}

fn detect_approval_model(text_lower: &str) -> ApprovalModel {
    // Arrow navigation prompt
    if text_lower.contains("arrow")
        || text_lower.contains("use arrow keys")
        || text_lower.contains("←")
        || text_lower.contains("→")
        || text_lower.contains("←/→")
    {
        return ApprovalModel::Arrow;
    }

    // Numbered options
    if (text_lower.contains("1.") && text_lower.contains("2."))
        || (text_lower.contains("1)") && text_lower.contains("2)"))
        || text_lower.contains("1. yes")
        || text_lower.contains("2. yes")
        || text_lower.contains("3. no")
        || text_lower.contains("allow once")
        || text_lower.contains("allow always")
        || text_lower.contains("yes, and don't ask")
        || text_lower.contains("don't ask again")
    {
        return ApprovalModel::Numbered;
    }

    // Yes/No prompts
    if text_lower.contains("[y/n]")
        || text_lower.contains("[y/N]")
        || text_lower.contains("(y/n)")
        || text_lower.contains("(yes/no)")
    {
        return ApprovalModel::YesNo;
    }

    ApprovalModel::None
}

fn is_tool_approval_prompt(text_lower: &str, model: ApprovalModel) -> bool {
    if model == ApprovalModel::None {
        return false;
    }

    // Claude Code specific patterns
    if text_lower.contains("do you want to run")
        || text_lower.contains("wants to use")
        || text_lower.contains("1. yes, proceed")
        || text_lower.contains("2. yes, and don't ask")
    {
        return true;
    }

    // Explicit tool approval keywords
    if text_lower.contains("tool")
        && (text_lower.contains("allow")
            || text_lower.contains("approve")
            || text_lower.contains("permission"))
    {
        return true;
    }

    // CLI-specific prompts (common patterns)
    if text_lower.contains("allow once") || text_lower.contains("allow always") {
        return true;
    }
    if text_lower.contains("yes, and don't ask") || text_lower.contains("don't ask again") {
        return true;
    }
    if text_lower.contains("do you want to allow") || text_lower.contains("allow this tool") {
        return true;
    }
    if text_lower.contains("permission")
        && (text_lower.contains("granted") || text_lower.contains("required"))
    {
        return true;
    }

    // Generic confirmation prompts are only treated as tool approval if options are explicit
    if text_lower.contains("do you want to proceed") || text_lower.contains("proceed?") {
        return model != ApprovalModel::None;
    }

    false
}

fn is_plan_approval_prompt(text_lower: &str, model: ApprovalModel) -> bool {
    if model == ApprovalModel::None {
        return false;
    }

    let has_plan = text_lower.contains("plan") || text_lower.contains("proposed plan");
    let has_approve = text_lower.contains("approve")
        || text_lower.contains("approval")
        || text_lower.contains("review");
    has_plan && has_approve
}

fn is_awaiting_response_prompt(text_lower: &str) -> bool {
    text_lower.contains("awaiting your response")
        || text_lower.contains("type your response")
        || text_lower.contains("enter your response")
        || text_lower.contains("press enter to continue")
        || text_lower.contains("hit enter to continue")
        || text_lower.contains("waiting for your input")
        || text_lower.contains("waiting for input")
        || text_lower.contains("choose an option")
        || text_lower.contains("enter your choice")
}

fn is_clarifying_question(text: &str, text_lower: &str) -> bool {
    // Use last line to reduce false positives
    if let Some(last_line) = text.lines().last() {
        let trimmed = last_line.trim();
        if trimmed.ends_with('?') {
            // Avoid misclassifying explicit approval prompts
            if !text_lower.contains("approve") && !text_lower.contains("allow") {
                return true;
            }
        }
    }
    false
}

pub fn detect_wait_event(input: &str, cli: CliType) -> Option<WaitEvent> {
    let normalized = strip_ansi_and_normalize(input);
    let tail = tail_chars(&normalized, 1200);
    // Focus on the last few lines to avoid stale prompt matches
    let tail_lines: String = tail
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let text_lower = tail_lines.to_lowercase();

    let approval_model = detect_approval_model(&text_lower);

    if is_plan_approval_prompt(&text_lower, approval_model) {
        let prompt = tail_chars(&tail, 300);
        return Some(WaitEvent {
            wait_type: WaitType::PlanApproval,
            approval_model,
            prompt_hash: hash_prompt(&prompt),
            prompt,
        });
    }

    if is_tool_approval_prompt(&text_lower, approval_model) {
        let prompt = tail_chars(&tail, 300);
        return Some(WaitEvent {
            wait_type: WaitType::ToolApproval,
            approval_model,
            prompt_hash: hash_prompt(&prompt),
            prompt,
        });
    }

    if is_clarifying_question(&tail_lines, &text_lower) {
        let prompt = tail_chars(&tail, 300);
        return Some(WaitEvent {
            wait_type: WaitType::ClarifyingQuestion,
            approval_model: cli.default_approval_model(),
            prompt_hash: hash_prompt(&prompt),
            prompt,
        });
    }

    if is_awaiting_response_prompt(&text_lower) {
        let prompt = tail_chars(&tail, 300);
        return Some(WaitEvent {
            wait_type: WaitType::AwaitingResponse,
            approval_model: cli.default_approval_model(),
            prompt_hash: hash_prompt(&prompt),
            prompt,
        });
    }

    None
}
