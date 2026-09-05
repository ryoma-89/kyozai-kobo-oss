use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const PATTERN_VERSION_LIMIT: i64 = 30;

fn clean_values(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.to_lowercase()))
        .map(str::to_string)
        .collect()
}

fn normalized_type(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "strategy".into()
    } else {
        value.to_string()
    }
}

fn normalized_relation(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn proposal_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn append_unique_text(base: &str, additions: &[String]) -> String {
    let mut lines: Vec<String> = base
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let mut seen: HashSet<String> = lines.iter().map(|line| proposal_key(line)).collect();
    for value in additions {
        let value = value.trim();
        if !value.is_empty() && seen.insert(proposal_key(value)) {
            lines.push(value.to_string());
        }
    }
    lines.join("\n")
}

fn validate_proposal(proposal: &PatternProposal) -> Result<(), String> {
    // カードに必ず要るのはタイトルと候補手法だけ。
    // 概要・状況・原則は、数式で言い切れない補足があるときだけ埋まればよい。
    if proposal.title.trim().is_empty() {
        return Err("定石候補のタイトルを入力してください".into());
    }
    let limits = [
        ("タイトル", proposal.title.as_str(), 60usize),
        ("概要", proposal.summary.as_str(), 300usize),
        ("状況", proposal.situation.as_str(), 300usize),
        ("原則", proposal.principle.as_str(), 400usize),
    ];
    for (label, value, max) in limits {
        if value.chars().count() > max {
            return Err(format!("定石候補の{}が長すぎます", label));
        }
    }
    if !matches!(
        proposal.pattern_type.as_str(),
        "strategy" | "technique" | "calculation_tip" | "check"
    ) {
        return Err("定石候補の種類が不正です".into());
    }
    if !matches!(
        proposal.source_type.as_str(),
        "solution_used"
            | "explanation_used"
            | "ai_inferred"
            | "image_import"
            | "ai_chat"
            | "manual"
    ) {
        return Err("定石候補の抽出根拠が不正です".into());
    }
    if proposal.strategies.is_empty() || proposal.strategies.len() > 6 {
        return Err("候補手法は1〜6件で指定してください".into());
    }
    for strategy in &proposal.strategies {
        if strategy.title.trim().is_empty()
            || strategy.title.chars().count() > 60
            || strategy.description.chars().count() > 400
            || strategy.condition.chars().count() > 200
            || strategy.reasoning.chars().count() > 200
        {
            return Err("候補手法の必須項目または文字数が不正です".into());
        }
    }
    for values in [
        &proposal.cautions,
        &proposal.domains,
        &proposal.goals,
        &proposal.operations,
        &proposal.structures,
        &proposal.situations,
        &proposal.tags,
    ] {
        if values.len() > 8 || values.iter().any(|value| value.chars().count() > 200) {
            return Err("定石候補の分類・注意事項が多すぎるか長すぎます".into());
        }
    }
    if proposal.raw_technique.chars().count() > 400
        || proposal.generalization_reason.chars().count() > 200
        || proposal.specificity_reason.chars().count() > 200
    {
        return Err("定石候補の抽象化に関する説明が長すぎます".into());
    }
    if proposal.search_concepts.len() > 8
        || proposal
            .search_concepts
            .iter()
            .any(|value| value.chars().count() > 30)
    {
        return Err("検索用の概念語が多すぎるか長すぎます".into());
    }
    if !(1..=4).contains(&proposal.specificity_level) {
        return Err("定石候補の抽象度は1〜4で指定してください".into());
    }
    if !proposal.reusability_score.is_finite() || !(0.0..=1.0).contains(&proposal.reusability_score)
    {
        return Err("定石候補の再利用可能性は0〜1で指定してください".into());
    }
    if !matches!(
        proposal.recommended_storage.as_str(),
        "new_pattern"
            | "child_pattern"
            | "example"
            | "candidate_strategy"
            | "merge_existing"
            | "duplicate"
            | "ignore"
    ) {
        return Err("定石候補の推奨保存方式が不正です".into());
    }
    if let Some(hint) = proposal.possible_parent_pattern.as_ref() {
        if hint.title.trim().is_empty()
            || hint.title.chars().count() > 60
            || hint.reason.chars().count() > 200
        {
            return Err("上位定石の候補が不正です".into());
        }
    }
    crate::ai::validate_pattern_proposal_language(proposal)?;
    Ok(())
}

fn proposal_strategy_inputs(proposal: &PatternProposal) -> Vec<PatternStrategyInput> {
    proposal
        .strategies
        .iter()
        .enumerate()
        .map(|(index, strategy)| PatternStrategyInput {
            id: None,
            parent_strategy_id: None,
            title: strategy.title.trim().to_string(),
            description: strategy.description.trim().to_string(),
            condition: strategy.condition.trim().to_string(),
            reasoning: strategy.reasoning.trim().to_string(),
            branch_label: String::new(),
            sort_order: index as i64 + 1,
        })
        .collect()
}

/// 既存定石のexamplesへ追記する行。元問題で実際に使われた具体的手法を残す。
fn proposal_example_lines(proposal: &PatternProposal) -> Vec<String> {
    let mut lines = Vec::new();
    let raw = proposal.raw_technique.trim();
    let title = proposal.title.trim();
    if !raw.is_empty() {
        lines.push(if title.is_empty() {
            raw.to_string()
        } else {
            format!("{title}：{raw}")
        });
    } else if !title.is_empty() {
        lines.push(title.to_string());
    }
    lines
}

/// 定石の作成経路。patterns.source_kind へ入れて後から追跡できるようにする。
fn source_kind_for_proposal(source_type: &str) -> &'static str {
    match source_type {
        "solution_used" | "explanation_used" => "problem_solution",
        "ai_inferred" => "problem_ai_inferred",
        "image_import" => "image_import",
        "ai_chat" => "ai_chat",
        _ => "manual",
    }
}

/// 由来を1行で残す。画像・チャット由来では巨大なデータを持たず、参照だけを書く。
fn source_note_for_proposal(problem_id: i64, source_type: &str, reference: &str) -> String {
    let label = match source_type {
        "solution_used" => "既存解答で使用",
        "explanation_used" => "既存解説で使用",
        "ai_inferred" => "AI推定",
        "image_import" => "画像から取り込み",
        "ai_chat" => "AI Chatで作成",
        _ => "手動で作成",
    };
    let reference = reference.trim();
    if problem_id > 0 {
        format!("Problem #{}から一般化（{}）", problem_id, label)
    } else if reference.is_empty() {
        label.to_string()
    } else {
        format!("{}（{}）", label, reference)
    }
}

fn tags_of(conn: &Connection, pattern_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT tag FROM pattern_tags WHERE pattern_id=?1 ORDER BY tag COLLATE NOCASE")?;
    let rows = stmt
        .query_map(params![pattern_id], |row| row.get(0))?
        .collect();
    rows
}

fn facet_values(
    conn: &Connection,
    pattern_id: i64,
    facet_type: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT value FROM pattern_facets
         WHERE pattern_id=?1 AND facet_type=?2 ORDER BY value COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map(params![pattern_id, facet_type], |row| row.get(0))?
        .collect();
    rows
}

fn facets_of(conn: &Connection, pattern_id: i64) -> rusqlite::Result<PatternFacets> {
    Ok(PatternFacets {
        domains: facet_values(conn, pattern_id, "domain")?,
        goals: facet_values(conn, pattern_id, "goal")?,
        operations: facet_values(conn, pattern_id, "operation")?,
        structures: facet_values(conn, pattern_id, "structure")?,
        situations: facet_values(conn, pattern_id, "situation")?,
    })
}

fn strategies_of(conn: &Connection, pattern_id: i64) -> rusqlite::Result<Vec<PatternStrategy>> {
    let mut stmt = conn.prepare(
        "SELECT id,pattern_id,parent_strategy_id,title,description,condition_text,reasoning,
                branch_label,sort_order
         FROM pattern_strategies WHERE pattern_id=?1 ORDER BY sort_order,id",
    )?;
    let rows = stmt
        .query_map(params![pattern_id], |row| {
            Ok(PatternStrategy {
                id: row.get(0)?,
                pattern_id: row.get(1)?,
                parent_strategy_id: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                condition: row.get(5)?,
                reasoning: row.get(6)?,
                branch_label: row.get(7)?,
                sort_order: row.get(8)?,
            })
        })?
        .collect();
    rows
}

fn related_patterns_of(
    conn: &Connection,
    pattern_id: i64,
) -> rusqlite::Result<Vec<PatternRelationView>> {
    let mut stmt = conn.prepare(
        "SELECT r.from_pattern_id,r.to_pattern_id,p.id,p.title,p.pattern_type,r.relation_type,'outgoing'
         FROM pattern_relations r JOIN patterns p ON p.id=r.to_pattern_id
         WHERE r.from_pattern_id=?1
         UNION ALL
         SELECT r.from_pattern_id,r.to_pattern_id,p.id,p.title,p.pattern_type,r.relation_type,'incoming'
         FROM pattern_relations r JOIN patterns p ON p.id=r.from_pattern_id
         WHERE r.to_pattern_id=?1
         ORDER BY 6,4",
    )?;
    let rows = stmt
        .query_map(params![pattern_id], |row| {
            Ok(PatternRelationView {
                from_pattern_id: row.get(0)?,
                to_pattern_id: row.get(1)?,
                pattern_id: row.get(2)?,
                title: row.get(3)?,
                pattern_type: row.get(4)?,
                relation_type: row.get(5)?,
                direction: row.get(6)?,
            })
        })?
        .collect();
    rows
}

fn related_problems_of(
    conn: &Connection,
    pattern_id: i64,
) -> Result<Vec<PatternProblemView>, String> {
    let rows: Vec<(i64, String, Option<i64>, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT p.id,p.title,p.bank_node_id,pp.relation_type
                 FROM problem_patterns pp JOIN problems p ON p.id=pp.problem_id
                 WHERE pp.pattern_id=?1 ORDER BY pp.relation_type,p.title,p.id",
            )
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![pattern_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    rows.into_iter()
        .map(|(problem_id, title, bank_node_id, relation_type)| {
            let bank_node_id = bank_node_id.unwrap_or_default();
            let bank_path = if bank_node_id > 0 {
                super::problems::bank_path(conn, bank_node_id).map_err(err_str)?
            } else {
                String::new()
            };
            Ok(PatternProblemView {
                problem_id,
                title,
                bank_node_id,
                bank_path,
                relation_type,
            })
        })
        .collect()
}

fn get_pattern_conn(conn: &Connection, id: i64) -> Result<PatternFull, String> {
    let mut pattern = conn
        .query_row(
            "SELECT id,uuid,title,summary,pattern_type,situation,principle,cautions,examples,
                    source_note,created_at,updated_at,version
             FROM patterns WHERE id=?1",
            params![id],
            |row| {
                Ok(PatternFull {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    pattern_type: row.get(4)?,
                    situation: row.get(5)?,
                    principle: row.get(6)?,
                    cautions: row.get(7)?,
                    examples: row.get(8)?,
                    source_note: row.get(9)?,
                    tags: vec![],
                    facets: PatternFacets::default(),
                    strategies: vec![],
                    related_patterns: vec![],
                    related_problems: vec![],
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    version: row.get(12)?,
                })
            },
        )
        .map_err(err_str)?;
    pattern.tags = tags_of(conn, id).map_err(err_str)?;
    pattern.facets = facets_of(conn, id).map_err(err_str)?;
    pattern.strategies = strategies_of(conn, id).map_err(err_str)?;
    pattern.related_patterns = related_patterns_of(conn, id).map_err(err_str)?;
    pattern.related_problems = related_problems_of(conn, id)?;
    Ok(pattern)
}

fn snapshot_of(conn: &Connection, pattern_id: i64) -> Result<PatternSnapshot, String> {
    let pattern = get_pattern_conn(conn, pattern_id)?;
    Ok(PatternSnapshot {
        version: pattern.version,
        uuid: pattern.uuid,
        title: pattern.title,
        summary: pattern.summary,
        pattern_type: pattern.pattern_type,
        situation: pattern.situation,
        principle: pattern.principle,
        cautions: pattern.cautions,
        examples: pattern.examples,
        source_note: pattern.source_note,
        tags: pattern.tags,
        facets: pattern.facets,
        strategies: pattern
            .strategies
            .into_iter()
            .map(|strategy| PatternStrategyInput {
                id: Some(strategy.id),
                parent_strategy_id: strategy.parent_strategy_id,
                title: strategy.title,
                description: strategy.description,
                condition: strategy.condition,
                reasoning: strategy.reasoning,
                branch_label: strategy.branch_label,
                sort_order: strategy.sort_order,
            })
            .collect(),
    })
}

/// 保存済みの定石を、解説・部品・教材へ貼れる tcolorbox のLaTeXにする。
/// 定石そのものは変更しない。
pub fn pattern_card_latex(state: &AppState, pattern_id: i64) -> Result<String, String> {
    let snapshot = pattern_snapshot(state, pattern_id)?;
    Ok(super::pattern_card::render_pattern_card(&snapshot))
}

/// 教材へ挿入するためのスナップショットを取る。canonical Patternは変更しない。
pub fn pattern_snapshot(state: &AppState, pattern_id: i64) -> Result<PatternSnapshot, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    snapshot_of(&conn, pattern_id)
}

fn save_version(conn: &Connection, pattern_id: i64) -> Result<(), String> {
    let snapshot = snapshot_of(conn, pattern_id)?;
    let version: i64 = conn
        .query_row(
            "SELECT version FROM patterns WHERE id=?1",
            params![pattern_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    let json = serde_json::to_string(&snapshot).map_err(err_str)?;
    conn.execute(
        "INSERT INTO pattern_versions(pattern_id,version,snapshot_json,saved_at)
         VALUES (?1,?2,?3,?4)",
        params![pattern_id, version, json, now_str()],
    )
    .map_err(err_str)?;
    conn.execute(
        "DELETE FROM pattern_versions WHERE pattern_id=?1 AND id NOT IN (
             SELECT id FROM pattern_versions WHERE pattern_id=?1 ORDER BY id DESC LIMIT ?2
         )",
        params![pattern_id, PATTERN_VERSION_LIMIT],
    )
    .map_err(err_str)?;
    Ok(())
}

fn set_tags(conn: &Connection, pattern_id: i64, tags: &[String]) -> Result<(), String> {
    conn.execute(
        "DELETE FROM pattern_tags WHERE pattern_id=?1",
        params![pattern_id],
    )
    .map_err(err_str)?;
    for tag in clean_values(tags) {
        conn.execute(
            "INSERT INTO pattern_tags(pattern_id,tag) VALUES (?1,?2)",
            params![pattern_id, tag],
        )
        .map_err(err_str)?;
    }
    Ok(())
}

fn set_facets(conn: &Connection, pattern_id: i64, facets: &PatternFacets) -> Result<(), String> {
    conn.execute(
        "DELETE FROM pattern_facets WHERE pattern_id=?1",
        params![pattern_id],
    )
    .map_err(err_str)?;
    for (facet_type, values) in [
        ("domain", &facets.domains),
        ("goal", &facets.goals),
        ("operation", &facets.operations),
        ("structure", &facets.structures),
        ("situation", &facets.situations),
    ] {
        for value in clean_values(values) {
            conn.execute(
                "INSERT INTO pattern_facets(pattern_id,facet_type,value) VALUES (?1,?2,?3)",
                params![pattern_id, facet_type, value],
            )
            .map_err(err_str)?;
        }
    }
    Ok(())
}

fn set_strategies(
    conn: &Connection,
    pattern_id: i64,
    strategies: &[PatternStrategyInput],
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM pattern_strategies WHERE pattern_id=?1",
        params![pattern_id],
    )
    .map_err(err_str)?;
    let mut id_map = HashMap::new();
    let mut inserted = Vec::new();
    for (index, strategy) in strategies.iter().enumerate() {
        let title = strategy.title.trim();
        if title.is_empty() {
            continue;
        }
        let actual_id = if let Some(id) = strategy.id {
            conn.execute(
                "INSERT INTO pattern_strategies
                 (id,pattern_id,parent_strategy_id,title,description,condition_text,reasoning,branch_label,sort_order)
                 VALUES (?1,?2,NULL,?3,?4,?5,?6,?7,?8)",
                params![
                    id,
                    pattern_id,
                    title,
                    strategy.description,
                    strategy.condition,
                    strategy.reasoning,
                    strategy.branch_label,
                    index as i64
                ],
            )
            .map_err(err_str)?;
            id_map.insert(id, id);
            id
        } else {
            conn.execute(
                "INSERT INTO pattern_strategies
                 (pattern_id,parent_strategy_id,title,description,condition_text,reasoning,branch_label,sort_order)
                 VALUES (?1,NULL,?2,?3,?4,?5,?6,?7)",
                params![
                    pattern_id,
                    title,
                    strategy.description,
                    strategy.condition,
                    strategy.reasoning,
                    strategy.branch_label,
                    index as i64
                ],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        };
        inserted.push((actual_id, strategy.parent_strategy_id));
    }
    for (actual_id, parent) in inserted {
        if let Some(parent) = parent.and_then(|id| id_map.get(&id).copied()) {
            conn.execute(
                "UPDATE pattern_strategies SET parent_strategy_id=?1 WHERE id=?2",
                params![parent, actual_id],
            )
            .map_err(err_str)?;
        }
    }
    Ok(())
}

fn solution_subject_from_root_name(name: &str) -> &'static str {
    if name.contains("物理") {
        "physics"
    } else if name.contains("化学") {
        "chemistry"
    } else if name.contains("生物") {
        "biology"
    } else if name.contains("英語") {
        "english"
    } else if name.contains("国語") {
        "japanese"
    } else if name.contains("情報") {
        "information"
    } else if name.contains("地理") || name.contains("歴史") || name.contains("公民") {
        "social_studies"
    } else {
        "mathematics"
    }
}

/// 保存済みProblemを正本として抽出ジョブを開始する。canonical Patternは変更しない。
pub fn start_pattern_extraction(
    state: &Arc<AppState>,
    problem_id: i64,
    style: Option<String>,
    instruction: Option<String>,
) -> Result<Value, String> {
    let problem = super::problems::get_problem(state, problem_id)?;
    let statement = if problem.statement_latex.trim().is_empty() {
        problem.statement_latex_two_column.trim()
    } else {
        problem.statement_latex.trim()
    };
    if statement.is_empty() {
        return Err("定石を抽出する問題文がありません".into());
    }
    let (root_name, bank_path) = {
        let conn = state.conn.lock().map_err(err_str)?;
        let root_name = conn
            .query_row(
                "WITH RECURSIVE ancestors(id,parent_id,name,depth) AS (
                 SELECT id,parent_id,name,0 FROM bank_nodes WHERE id=?1
                 UNION ALL
                 SELECT parent.id,parent.parent_id,parent.name,ancestors.depth+1
                 FROM bank_nodes parent JOIN ancestors ON ancestors.parent_id=parent.id
             ) SELECT name FROM ancestors ORDER BY depth DESC LIMIT 1",
                params![problem.bank_node_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default();
        let bank_path = super::problems::bank_path(&conn, problem.bank_node_id).map_err(err_str)?;
        (root_name, bank_path)
    };
    let has_answer = !problem.answer_latex.trim().is_empty();
    let has_explanation = !problem.explanation_latex.trim().is_empty();
    let input = [
        "【問題タイトル】".to_string(),
        problem.title.clone(),
        "".into(),
        "【問題文】".into(),
        statement.to_string(),
        "".into(),
        "【既存解答】".into(),
        if has_answer {
            problem.answer_latex.clone()
        } else {
            "（未作成。必要ならAIで解法を推定し、sourceTypeをai_inferredにすること）".into()
        },
        "".into(),
        "【既存解説】".into(),
        if has_explanation {
            problem.explanation_latex.clone()
        } else {
            "（未作成）".into()
        },
        "".into(),
        "【既存タグ・整理情報】".into(),
        format!("階層: {}\nタグ: {}", bank_path, problem.tags.join(" / ")),
    ]
    .join("\n");
    crate::ai::create_job(
        state,
        crate::ai::CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("pattern_extraction".into()),
            options: Some(json!({
                "patternExtractionSourceVersion": problem.version,
                "patternExtractionProblemId": problem.id,
                "hasAnswer": has_answer,
                "hasExplanation": has_explanation,
                "solutionSubject": solution_subject_from_root_name(&root_name),
                // やり直し時の方針。省略時は standard（初回と同じ挙動）。
                "patternExtractionStyle": style.unwrap_or_else(|| "standard".into()),
                "patternExtractionInstruction": instruction.unwrap_or_default(),
                "hideFromHistory": true
            })),
            input_text: Some(input),
            input_names: vec![],
            target_entity_type: Some("problem".into()),
            target_entity_id: Some(problem.id),
            target_field: Some("pattern_extraction".into()),
        },
    )
}

/// 既存の定石をAIへ渡し、指示どおりに書き直した候補を作るジョブを開始する。
/// canonical Patternはこの時点では変更しない。
pub fn start_pattern_edit(
    state: &Arc<AppState>,
    pattern_id: i64,
    instruction: String,
) -> Result<Value, String> {
    let instruction = instruction.trim().to_string();
    if instruction.is_empty() {
        return Err("AIへの編集指示を入力してください".into());
    }
    let proposal = pattern_edit_proposal(state, pattern_id)?;
    let input = serde_json::to_string_pretty(&proposal).map_err(err_str)?;
    crate::ai::create_job(
        state,
        crate::ai::CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("pattern_edit".into()),
            options: Some(json!({
                "patternEditPatternId": pattern_id,
                "patternEditInstruction": instruction,
                "patternEditSourceVersion": current_pattern_version(state, pattern_id)?,
                "solutionSubject": "mathematics",
                "hideFromHistory": true
            })),
            input_text: Some(input),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
}

fn current_pattern_version(state: &AppState, pattern_id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.query_row(
        "SELECT version FROM patterns WHERE id=?1",
        params![pattern_id],
        |row| row.get(0),
    )
    .map_err(|_| "定石が見つかりません".to_string())
}

/// 既存の定石を、AIが読み書きするProposalの形へ写す。
pub fn pattern_edit_proposal(state: &AppState, pattern_id: i64) -> Result<PatternProposal, String> {
    let pattern = get_pattern(state, pattern_id)?;
    Ok(PatternProposal {
        proposal_id: format!("pattern-{pattern_id}"),
        raw_technique: String::new(),
        title: pattern.title,
        pattern_type: pattern.pattern_type,
        summary: pattern.summary,
        situation: pattern.situation,
        principle: pattern.principle,
        strategies: pattern
            .strategies
            .into_iter()
            .enumerate()
            .map(|(index, strategy)| PatternProposalStrategy {
                title: strategy.title,
                description: strategy.description,
                condition: strategy.condition,
                reasoning: strategy.reasoning,
                sort_order: index as i64 + 1,
            })
            .collect(),
        cautions: pattern
            .cautions
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        domains: pattern.facets.domains,
        goals: pattern.facets.goals,
        operations: pattern.facets.operations,
        structures: pattern.facets.structures,
        situations: pattern.facets.situations,
        tags: pattern.tags,
        source_type: "manual".into(),
        matched_pattern_id: None,
        matched_pattern_title: None,
        similarity_reason: String::new(),
        action_recommendation: "create_new".into(),
        generalization_reason: String::new(),
        specificity_level: 0,
        reusability_score: 0.0,
        search_concepts: vec![],
        is_overly_specific: false,
        is_overly_general: false,
        specificity_reason: String::new(),
        possible_parent_pattern: None,
        generalization_decision: String::new(),
        recommended_storage: String::new(),
        generalization_pass_count: 0,
    })
}

/// AIが書き直した候補を、ユーザーの承認後に既存の定石へ反映する。
/// 既存のupdate_patternを通すので、変更履歴も楽観的ロックもそのまま効く。
pub fn apply_pattern_edit(
    state: &AppState,
    pattern_id: i64,
    expected_version: Option<i64>,
    mut proposal: PatternProposal,
) -> Result<i64, String> {
    crate::ai::normalize_pattern_proposal_language(&mut proposal);
    crate::ai::normalize_pattern_proposal_defaults(&mut proposal);
    validate_proposal(&proposal)?;
    let current = get_pattern(state, pattern_id)?;
    update_pattern(
        state,
        PatternUpdate {
            id: pattern_id,
            expected_version,
            title: proposal.title.trim().to_string(),
            summary: proposal.summary.trim().to_string(),
            pattern_type: normalized_type(&proposal.pattern_type),
            situation: proposal.situation.trim().to_string(),
            principle: proposal.principle.trim().to_string(),
            cautions: clean_values(&proposal.cautions).join("\n"),
            // 例と出典はAIの編集対象にしないので、保存済みの内容をそのまま保つ。
            examples: current.examples,
            source_note: current.source_note,
            tags: proposal.tags.clone(),
            facets: PatternFacets {
                domains: proposal.domains.clone(),
                goals: proposal.goals.clone(),
                operations: proposal.operations.clone(),
                structures: proposal.structures.clone(),
                situations: proposal.situations.clone(),
            },
            strategies: proposal_strategy_inputs(&proposal),
        },
    )
}

/// 教材写真などの画像から定石候補を取り込むジョブを開始する。
/// canonical Patternは作らず、Proposalだけを生成する。
pub fn start_pattern_image_import(
    state: &Arc<AppState>,
    input_names: Vec<String>,
    note: Option<String>,
) -> Result<Value, String> {
    if input_names.is_empty() {
        return Err("取り込む画像を選択してください".into());
    }
    let note = note.unwrap_or_default();
    if note.chars().count() > 1000 {
        return Err("画像への補足は1000文字までです".into());
    }
    let mut lines = vec!["---- 取り込む画像 ----".to_string()];
    for (index, name) in input_names.iter().enumerate() {
        lines.push(format!("画像{}: {}", index + 1, name));
    }
    if !note.trim().is_empty() {
        lines.push(String::new());
        lines.push("---- ユーザーからの補足 ----".into());
        lines.push(
            "読み取りの手掛かりとして扱い、画像から読めない事実を推測で補わないでください。".into(),
        );
        lines.push(note.trim().to_string());
    }
    crate::ai::create_job(
        state,
        crate::ai::CreateJobPayload {
            source_type: "image".into(),
            conversion_mode: Some("pattern_image_import".into()),
            options: Some(json!({
                "solutionSubject": "mathematics",
                "hideFromHistory": true
            })),
            input_text: Some(lines.join("\n")),
            input_names,
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
}

/// 抽出済みのProposal1件だけをAIへ渡し、さらに一般化した候補を得るジョブを開始する。
/// Problem全体を再解析せず、canonical Patternも変更しない。
pub fn start_pattern_generalization(
    state: &Arc<AppState>,
    problem_id: i64,
    mut proposal: PatternProposal,
) -> Result<Value, String> {
    if problem_id <= 0 {
        return Err("抽出元Problemが不正です".into());
    }
    crate::ai::normalize_pattern_proposal_language(&mut proposal);
    crate::ai::normalize_pattern_proposal_defaults(&mut proposal);
    validate_proposal(&proposal)?;
    let pass_count = proposal.generalization_pass_count.max(0);
    if pass_count >= crate::ai::PATTERN_GENERALIZATION_MAX_PASSES {
        return Err(format!(
            "これ以上は一般化できません（最大{}回）",
            crate::ai::PATTERN_GENERALIZATION_MAX_PASSES
        ));
    }
    // 分類結果はライブラリの現状に依存するため、一般化の入力からは外す。
    proposal.matched_pattern_id = None;
    proposal.matched_pattern_title = None;
    proposal.similarity_reason = String::new();
    proposal.action_recommendation = String::new();
    let input = serde_json::to_string_pretty(&proposal).map_err(err_str)?;
    crate::ai::create_job(
        state,
        crate::ai::CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("pattern_generalization".into()),
            options: Some(json!({
                "patternGeneralizationProblemId": problem_id,
                "patternGeneralizationPassCount": pass_count,
                "hideFromHistory": true
            })),
            input_text: Some(input),
            input_names: vec![],
            target_entity_type: Some("problem".into()),
            target_entity_id: Some(problem_id),
            target_field: Some("pattern_generalization".into()),
        },
    )
}

fn overlap_count(left: &[String], right: &[String]) -> i64 {
    let right: HashSet<String> = right.iter().map(|value| proposal_key(value)).collect();
    left.iter()
        .map(|value| proposal_key(value))
        .filter(|value| !value.is_empty() && right.contains(value))
        .count() as i64
}

/// 語順や助詞の違いを吸収するため、文字2-gramの集合で見出しを比べる。
/// 短い方をどれだけ含んでいるかを見るので、長いタイトルへの言い換えも拾える。
fn bigram_containment(left: &str, right: &str) -> f64 {
    fn bigrams(text: &str) -> HashSet<String> {
        let chars: Vec<char> = proposal_key(text).chars().collect();
        chars
            .windows(2)
            .map(|window| window.iter().collect::<String>())
            .collect()
    }
    let left = bigrams(left);
    let right = bigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let shared = left.intersection(&right).count() as f64;
    shared / left.len().min(right.len()) as f64
}

/// 既存定石側の検索対象テキストを、概念語の照合用にまとめる。
fn pattern_haystack(pattern: &PatternFull) -> String {
    let mut text = String::new();
    for value in [
        &pattern.title,
        &pattern.summary,
        &pattern.situation,
        &pattern.principle,
        &pattern.cautions,
        &pattern.examples,
    ] {
        text.push_str(value);
    }
    for strategy in &pattern.strategies {
        text.push_str(&strategy.title);
        text.push_str(&strategy.description);
        text.push_str(&strategy.condition);
    }
    text.push_str(&pattern.tags.join(""));
    for values in [
        &pattern.facets.domains,
        &pattern.facets.goals,
        &pattern.facets.operations,
        &pattern.facets.structures,
        &pattern.facets.situations,
    ] {
        text.push_str(&values.join(""));
    }
    proposal_key(&text)
}

fn proposal_similarity(proposal: &PatternProposal, pattern: &PatternFull) -> (i64, Vec<String>) {
    let proposal_title = proposal_key(&proposal.title);
    let pattern_title = proposal_key(&pattern.title);
    let mut score = 0;
    let mut reasons = Vec::new();
    if proposal_title == pattern_title && !proposal_title.is_empty() {
        score += 100;
        reasons.push("タイトルが一致".to_string());
    } else if proposal_title.len() >= 4
        && pattern_title.len() >= 4
        && (proposal_title.contains(&pattern_title) || pattern_title.contains(&proposal_title))
    {
        score += 8;
        reasons.push("タイトルの中心語が共通".to_string());
    }
    let proposal_summary = proposal_key(&proposal.summary);
    let pattern_summary = proposal_key(&pattern.summary);
    if proposal_summary.len() >= 12
        && pattern_summary.len() >= 12
        && (proposal_summary.contains(&pattern_summary)
            || pattern_summary.contains(&proposal_summary))
    {
        score += 4;
        reasons.push("概要が近い".to_string());
    }
    let proposal_situation = proposal_key(&proposal.situation);
    let pattern_situation = proposal_key(&pattern.situation);
    if proposal_situation.len() >= 8
        && pattern_situation.len() >= 8
        && (proposal_situation.contains(&pattern_situation)
            || pattern_situation.contains(&proposal_situation))
    {
        score += 4;
        reasons.push("想起すべき状況が近い".to_string());
    }
    let tag_overlap = overlap_count(&proposal.tags, &pattern.tags);
    if tag_overlap > 0 {
        score += (tag_overlap * 2).min(6);
        reasons.push(format!("タグ{}件が共通", tag_overlap));
    }
    let facet_overlap = overlap_count(&proposal.domains, &pattern.facets.domains)
        + overlap_count(&proposal.goals, &pattern.facets.goals)
        + overlap_count(&proposal.operations, &pattern.facets.operations)
        + overlap_count(&proposal.structures, &pattern.facets.structures)
        + overlap_count(&proposal.situations, &pattern.facets.situations);
    if facet_overlap > 0 {
        score += facet_overlap.min(6);
        reasons.push(format!("分類軸{}件が共通", facet_overlap));
    }
    let existing_strategy_titles: Vec<String> = pattern
        .strategies
        .iter()
        .map(|strategy| strategy.title.clone())
        .collect();
    let proposed_strategy_titles: Vec<String> = proposal
        .strategies
        .iter()
        .map(|strategy| strategy.title.clone())
        .collect();
    let strategy_overlap = overlap_count(&proposed_strategy_titles, &existing_strategy_titles);
    if strategy_overlap > 0 {
        score += (strategy_overlap * 7).min(14);
        reasons.push(format!("候補手法{}件が一致", strategy_overlap));
    }
    // searchConceptsは既存定石を引き当てるために生成した語なので、
    // 分類の類似度にも反映する。これがないと、検索で拾えた上位定石を
    // タイトルの字面が違うというだけで捨ててしまう。
    let haystack = pattern_haystack(pattern);
    let concept_hits = proposal
        .search_concepts
        .iter()
        .map(|concept| proposal_key(concept))
        .filter(|concept| concept.chars().count() >= 2 && haystack.contains(concept.as_str()))
        .count() as i64;
    // 「求める」のような一般語は1件だけ一致することがあるため、1件では弱い証拠として扱う。
    if concept_hits >= 2 {
        score += (concept_hits * 2).min(8);
        reasons.push(format!("検索概念{}件が一致", concept_hits));
    } else if concept_hits == 1 {
        score += 1;
    }
    // 一般化されたタイトルは語順や言い回しが変わるため、字面の包含ではなく語の重なりで見る。
    let title_ratio = bigram_containment(&proposal.title, &pattern.title);
    // 短いタイトルでは「を求める」等の語尾だけで2割前後が重なるため、下位の段は弱くする。
    let (title_score, title_reason) = if title_ratio >= 0.5 {
        (10, Some("タイトルの語がほぼ一致"))
    } else if title_ratio >= 0.35 {
        (6, Some("タイトルの語が広く共通"))
    } else if title_ratio >= 0.25 {
        (2, Some("タイトルの語が部分的に共通"))
    } else {
        (0, None)
    };
    if let Some(reason) = title_reason {
        score += title_score;
        reasons.push(reason.to_string());
    }
    // AIが「これの特殊化だ」と述べた上位定石が既にライブラリにあるなら、
    // 分野語やタグが重ならなくてもその既存定石を候補として扱う。
    if let Some(hint) = proposal.possible_parent_pattern.as_ref() {
        let hint_ratio = bigram_containment(&hint.title, &pattern.title);
        if hint_ratio >= 0.5 {
            score += 8;
            reasons.push("AIが挙げた上位定石と一致".to_string());
        } else if hint_ratio >= 0.35 {
            score += 4;
            reasons.push("AIが挙げた上位定石と語が共通".to_string());
        }
    }
    (score, reasons)
}

/// AIが抽出した候補ごとに、既存SQLite検索のTop-Kだけを比較して推奨を付与する。
/// 全PatternをAIやFrontendへ送らない。
pub fn classify_pattern_proposals(
    state: &AppState,
    result: &mut PatternExtractionResult,
) -> Result<(), String> {
    for proposal in &mut result.patterns {
        crate::ai::normalize_pattern_proposal_language(proposal);
        crate::ai::normalize_pattern_proposal_defaults(proposal);
        validate_proposal(proposal)?;
        // 生成されたtitleそのものだけで検索すると、具体的な候補ほど一般的な既存定石へ
        // 当たらなくなる。AIが返した一般化キーワードを先に使う。
        let mut terms: Vec<String> = proposal.search_concepts.iter().take(8).cloned().collect();
        if let Some(parent) = proposal.possible_parent_pattern.as_ref() {
            terms.push(parent.title.clone());
        }
        terms.push(proposal.title.clone());
        terms.push(proposal.summary.clone());
        terms.push(proposal.situation.clone());
        terms.push(proposal.principle.clone());
        terms.extend(
            proposal
                .strategies
                .iter()
                .take(3)
                .map(|value| value.title.clone()),
        );
        terms.extend(proposal.tags.iter().take(3).cloned());
        terms.extend(proposal.domains.iter().take(2).cloned());
        terms.extend(proposal.operations.iter().take(2).cloned());
        let mut candidate_ids = HashSet::new();
        for term in clean_values(&terms).into_iter().take(20) {
            for candidate in search_patterns(
                state,
                PatternSearchQuery {
                    text: term,
                    limit: Some(10),
                    ..Default::default()
                },
            )? {
                candidate_ids.insert(candidate.id);
                if candidate_ids.len() >= 30 {
                    break;
                }
            }
            if candidate_ids.len() >= 30 {
                break;
            }
        }

        // LIKE検索は長い文をそのまま渡しても何も引き当てられない。
        // タイトル同士・上位定石のヒントとの語の重なりでも候補を集める。
        if candidate_ids.len() < 30 {
            for summary in search_patterns(
                state,
                PatternSearchQuery {
                    // 個人利用の規模を想定し、直近200件のタイトルだけを走査する。
                    limit: Some(200),
                    ..Default::default()
                },
            )? {
                if candidate_ids.len() >= 30 {
                    break;
                }
                let by_title = bigram_containment(&proposal.title, &summary.title) >= 0.35;
                let by_hint = proposal
                    .possible_parent_pattern
                    .as_ref()
                    .is_some_and(|hint| bigram_containment(&hint.title, &summary.title) >= 0.5);
                if by_title || by_hint {
                    candidate_ids.insert(summary.id);
                }
            }
        }

        let mut best: Option<(i64, PatternFull, Vec<String>)> = None;
        for id in candidate_ids {
            let candidate = get_pattern(state, id)?;
            let (score, reasons) = proposal_similarity(proposal, &candidate);
            if best
                .as_ref()
                .is_none_or(|(best_score, _, _)| score > *best_score)
            {
                best = Some((score, candidate, reasons));
            }
        }
        let Some((score, matched, reasons)) = best.filter(|(score, _, _)| *score >= 5) else {
            proposal.matched_pattern_id = None;
            proposal.matched_pattern_title = None;
            proposal.similarity_reason = "近い既存定石は見つかりませんでした".into();
            // 上位定石が既存にない以上、child_pattern等は選べないため新規作成へ寄せる。
            proposal.action_recommendation = if proposal.recommended_storage == "ignore" {
                "ignore".into()
            } else {
                "create_new".into()
            };
            continue;
        };
        proposal.matched_pattern_id = Some(matched.id);
        proposal.matched_pattern_title = Some(matched.title.clone());
        proposal.similarity_reason = reasons.join(" / ");
        let existing_strategy_keys: HashSet<String> = matched
            .strategies
            .iter()
            .map(|strategy| proposal_key(&strategy.title))
            .collect();
        let has_new_strategy = proposal
            .strategies
            .iter()
            .any(|strategy| !existing_strategy_keys.contains(&proposal_key(&strategy.title)));
        let existing_cautions = proposal_key(&matched.cautions);
        let has_new_caution = proposal
            .cautions
            .iter()
            .any(|caution| !existing_cautions.contains(&proposal_key(caution)));
        // 既存定石が、AIの言う上位定石と同じものを指しているなら特殊化として扱える。
        let matched_is_parent_hint = proposal
            .possible_parent_pattern
            .as_ref()
            .filter(|hint| hint.title.chars().count() >= 4)
            .is_some_and(|hint| bigram_containment(&hint.title, &matched.title) >= 0.5);
        let score_based = if score >= 14 {
            "merge_into_existing"
        } else if has_new_strategy {
            "add_candidate_to_existing"
        } else if has_new_caution {
            "add_caution_to_existing"
        } else {
            "duplicate"
        };
        // AIが「独立させず既存へ足す」と判断した候補まで、上位ヒントの一致だけで
        // 新規作成へ引き上げない。上書きするのは新規寄りの判断のときだけにする。
        let parent_hint_applies = matched_is_parent_hint
            && matches!(
                proposal.recommended_storage.as_str(),
                "new_pattern" | "child_pattern"
            );
        proposal.action_recommendation = if score >= 100 {
            "duplicate"
        } else if parent_hint_applies {
            "create_child_pattern"
        } else {
            // AIの保存方式の提案は、既存定石が実在する場合だけ採用する。
            match proposal.recommended_storage.as_str() {
                "example" => "add_example_to_existing",
                "candidate_strategy" if has_new_strategy => "add_candidate_to_existing",
                "candidate_strategy" => "add_example_to_existing",
                // AIが具体的な上位定石を挙げているのに、見つかった既存定石がそれと別物なら、
                // その定石の特殊化にはできない。上位を挙げていない場合は、見つかった定石を
                // 上位とみなす判断なのでそのまま採用する。
                "child_pattern"
                    if proposal.possible_parent_pattern.is_none() || matched_is_parent_hint =>
                {
                    "create_child_pattern"
                }
                "child_pattern" => score_based,
                "merge_existing" => "merge_into_existing",
                "duplicate" => "duplicate",
                "ignore" => "ignore",
                _ => score_based,
            }
        }
        .into();
    }
    Ok(())
}

pub fn apply_pattern_proposal(
    state: &AppState,
    mut payload: ApplyPatternProposalPayload,
) -> Result<ApplyPatternProposalResult, String> {
    crate::ai::normalize_pattern_proposal_language(&mut payload.proposal);
    crate::ai::normalize_pattern_proposal_defaults(&mut payload.proposal);
    validate_proposal(&payload.proposal)?;
    if payload.problem_id < 0 {
        return Err("抽出元Problemが不正です".into());
    }
    // Problemに紐づかない経路（画像・AI Chat・手動）では、Problemとの関連付けもできない。
    let has_source_problem = payload.problem_id > 0;
    if !has_source_problem && payload.link_relation_type.is_some() {
        return Err("抽出元Problemがない候補は、Problemと関連付けできません".into());
    }
    if !has_source_problem
        && matches!(
            payload.proposal.source_type.as_str(),
            "solution_used" | "explanation_used" | "ai_inferred"
        )
    {
        return Err("Problem由来の抽出根拠には抽出元Problemが必要です".into());
    }
    if payload.source_reference.chars().count() > 300 {
        return Err("由来の参照が長すぎます".into());
    }
    if !matches!(
        payload.action.as_str(),
        "create_new"
            | "create_child_pattern"
            | "merge_into_existing"
            | "add_candidate_to_existing"
            | "add_caution_to_existing"
            | "add_example_to_existing"
            | "link_existing"
    ) {
        return Err("定石候補の反映方法が不正です".into());
    }
    if let Some(relation) = payload.link_relation_type.as_deref() {
        if !matches!(relation, "applicable" | "used") {
            return Err("Problemとの関連種別が不正です".into());
        }
    }

    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    if has_source_problem {
        let source_content: Option<(String, String)> = tx
            .query_row(
                "SELECT answer_latex,explanation_latex FROM problems WHERE id=?1",
                params![payload.problem_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(err_str)?;
        let Some((answer_latex, explanation_latex)) = source_content else {
            return Err("抽出元Problemが見つかりません".into());
        };
        if payload.proposal.source_type == "solution_used" && answer_latex.trim().is_empty() {
            return Err("解答がないProblemを『既存解答で使用』として保存できません".into());
        }
        if payload.proposal.source_type == "explanation_used" && explanation_latex.trim().is_empty()
        {
            return Err("解説がないProblemを『既存解説で使用』として保存できません".into());
        }
    }
    let provenance = source_note_for_proposal(
        payload.problem_id,
        &payload.proposal.source_type,
        &payload.source_reference,
    );
    let created = matches!(
        payload.action.as_str(),
        "create_new" | "create_child_pattern"
    );
    // 特殊化として作る場合だけ、上位となる既存定石を先に確定させる。
    let parent_pattern_id = if payload.action == "create_child_pattern" {
        let parent = payload
            .parent_pattern_id
            .or(payload.target_pattern_id)
            .or(payload.proposal.matched_pattern_id)
            .ok_or("上位となる既存定石を選択してください")?;
        get_pattern_conn(&tx, parent)?;
        Some(parent)
    } else {
        None
    };
    let pattern_id = if created {
        let now = now_str();
        tx.execute(
            "INSERT INTO patterns(uuid,title,summary,pattern_type,situation,principle,cautions,examples,
                                  source_note,source_kind,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'',?8,?9,?10,?10)",
            params![
                uuid::Uuid::new_v4().to_string(),
                payload.proposal.title.trim(),
                payload.proposal.summary.trim(),
                normalized_type(&payload.proposal.pattern_type),
                payload.proposal.situation.trim(),
                payload.proposal.principle.trim(),
                clean_values(&payload.proposal.cautions).join("\n"),
                provenance,
                source_kind_for_proposal(&payload.proposal.source_type),
                now
            ],
        )
        .map_err(err_str)?;
        let id = tx.last_insert_rowid();
        set_tags(&tx, id, &payload.proposal.tags)?;
        set_facets(
            &tx,
            id,
            &PatternFacets {
                domains: payload.proposal.domains.clone(),
                goals: payload.proposal.goals.clone(),
                operations: payload.proposal.operations.clone(),
                structures: payload.proposal.structures.clone(),
                situations: payload.proposal.situations.clone(),
            },
        )?;
        set_strategies(&tx, id, &proposal_strategy_inputs(&payload.proposal))?;
        if let Some(parent) = parent_pattern_id {
            // 上位→下位はspecialization、下位→上位はgeneralizationの1組で表す。
            for (from, to, relation) in [
                (parent, id, "specialization"),
                (id, parent, "generalization"),
            ] {
                tx.execute(
                    "INSERT OR IGNORE INTO pattern_relations
                     (from_pattern_id,to_pattern_id,relation_type,created_at) VALUES (?1,?2,?3,?4)",
                    params![from, to, relation, now_str()],
                )
                .map_err(err_str)?;
            }
        }
        id
    } else {
        let target = payload
            .target_pattern_id
            .or(payload.proposal.matched_pattern_id)
            .ok_or("追記先の既存定石を選択してください")?;
        if payload.action != "link_existing" {
            let current = get_pattern_conn(&tx, target)?;
            save_version(&tx, target)?;
            let mut strategies: Vec<PatternStrategyInput> = current
                .strategies
                .iter()
                .map(|strategy| PatternStrategyInput {
                    id: Some(strategy.id),
                    parent_strategy_id: strategy.parent_strategy_id,
                    title: strategy.title.clone(),
                    description: strategy.description.clone(),
                    condition: strategy.condition.clone(),
                    reasoning: strategy.reasoning.clone(),
                    branch_label: strategy.branch_label.clone(),
                    sort_order: strategy.sort_order,
                })
                .collect();
            if matches!(
                payload.action.as_str(),
                "merge_into_existing" | "add_candidate_to_existing"
            ) {
                let mut seen: HashSet<String> = strategies
                    .iter()
                    .map(|strategy| proposal_key(&strategy.title))
                    .collect();
                for mut strategy in proposal_strategy_inputs(&payload.proposal) {
                    if seen.insert(proposal_key(&strategy.title)) {
                        strategy.sort_order = strategies.len() as i64 + 1;
                        strategies.push(strategy);
                    }
                }
            }
            let cautions = if matches!(
                payload.action.as_str(),
                "merge_into_existing" | "add_caution_to_existing"
            ) {
                append_unique_text(&current.cautions, &payload.proposal.cautions)
            } else {
                current.cautions.clone()
            };
            // 独立した定石にするほどではない知識は、既存定石の具体例として残す。
            let examples = if matches!(
                payload.action.as_str(),
                "merge_into_existing" | "add_example_to_existing"
            ) {
                append_unique_text(
                    &current.examples,
                    &proposal_example_lines(&payload.proposal),
                )
            } else {
                current.examples.clone()
            };
            let source_note = append_unique_text(&current.source_note, &[provenance]);
            tx.execute(
                "UPDATE patterns SET cautions=?1,examples=?2,source_note=?3,updated_at=?4,version=version+1 WHERE id=?5",
                params![cautions, examples, source_note, now_str(), target],
            )
            .map_err(err_str)?;
            if payload.action == "merge_into_existing" {
                let mut tags = current.tags.clone();
                tags.extend(payload.proposal.tags.clone());
                set_tags(&tx, target, &tags)?;
                let mut facets = current.facets.clone();
                facets.domains.extend(payload.proposal.domains.clone());
                facets.goals.extend(payload.proposal.goals.clone());
                facets
                    .operations
                    .extend(payload.proposal.operations.clone());
                facets
                    .structures
                    .extend(payload.proposal.structures.clone());
                facets
                    .situations
                    .extend(payload.proposal.situations.clone());
                set_facets(&tx, target, &facets)?;
            }
            set_strategies(&tx, target, &strategies)?;
        } else {
            // link_existingでも対象の存在は確認する。
            get_pattern_conn(&tx, target)?;
        }
        target
    };

    let linked = if let Some(relation) = payload.link_relation_type {
        tx.execute(
            "INSERT INTO problem_patterns(problem_id,pattern_id,relation_type,created_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(problem_id,pattern_id) DO UPDATE SET relation_type=excluded.relation_type",
            params![payload.problem_id, pattern_id, relation, now_str()],
        )
        .map_err(err_str)?;
        true
    } else {
        false
    };
    tx.commit().map_err(err_str)?;
    Ok(ApplyPatternProposalResult {
        pattern_id,
        action: payload.action,
        created,
        linked,
    })
}

pub fn create_pattern(
    state: &AppState,
    title: String,
    pattern_type: String,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let title = if title.trim().is_empty() {
        "新しい定石".to_string()
    } else {
        title.trim().to_string()
    };
    let now = now_str();
    conn.execute(
        "INSERT INTO patterns(uuid,title,pattern_type,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?4)",
        params![
            uuid::Uuid::new_v4().to_string(),
            title,
            normalized_type(&pattern_type),
            now
        ],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub fn get_pattern(state: &AppState, id: i64) -> Result<PatternFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    get_pattern_conn(&conn, id)
}

pub fn update_pattern(state: &AppState, payload: PatternUpdate) -> Result<i64, String> {
    if payload.title.trim().is_empty() {
        return Err("定石のタイトルを入力してください".into());
    }
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let current: i64 = tx
        .query_row(
            "SELECT version FROM patterns WHERE id=?1",
            params![payload.id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    if payload
        .expected_version
        .is_some_and(|expected| expected != current)
    {
        return Err(format!("CONFLICT:{}", current));
    }
    save_version(&tx, payload.id)?;
    tx.execute(
        "UPDATE patterns SET title=?1,summary=?2,pattern_type=?3,situation=?4,principle=?5,
                cautions=?6,examples=?7,source_note=?8,updated_at=?9,version=version+1
         WHERE id=?10",
        params![
            payload.title.trim(),
            payload.summary,
            normalized_type(&payload.pattern_type),
            payload.situation,
            payload.principle,
            payload.cautions,
            payload.examples,
            payload.source_note,
            now_str(),
            payload.id
        ],
    )
    .map_err(err_str)?;
    set_tags(&tx, payload.id, &payload.tags)?;
    set_facets(&tx, payload.id, &payload.facets)?;
    set_strategies(&tx, payload.id, &payload.strategies)?;
    tx.commit().map_err(err_str)?;
    Ok(current + 1)
}

pub fn duplicate_pattern(state: &AppState, id: i64) -> Result<i64, String> {
    let mut snapshot = {
        let conn = state.conn.lock().map_err(err_str)?;
        snapshot_of(&conn, id)?
    };
    snapshot.uuid = uuid::Uuid::new_v4().to_string();
    snapshot.title = format!("{} (コピー)", snapshot.title);
    for strategy in &mut snapshot.strategies {
        strategy.id = None;
        strategy.parent_strategy_id = None;
    }
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let now = now_str();
    tx.execute(
        "INSERT INTO patterns(uuid,title,summary,pattern_type,situation,principle,cautions,examples,
                              source_note,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
        params![
            snapshot.uuid,
            snapshot.title,
            snapshot.summary,
            snapshot.pattern_type,
            snapshot.situation,
            snapshot.principle,
            snapshot.cautions,
            snapshot.examples,
            snapshot.source_note,
            now
        ],
    )
    .map_err(err_str)?;
    let new_id = tx.last_insert_rowid();
    set_tags(&tx, new_id, &snapshot.tags)?;
    set_facets(&tx, new_id, &snapshot.facets)?;
    set_strategies(&tx, new_id, &snapshot.strategies)?;
    tx.commit().map_err(err_str)?;
    Ok(new_id)
}

pub fn get_pattern_delete_impact(
    state: &AppState,
    pattern_id: i64,
) -> Result<PatternDeleteImpact, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM patterns WHERE id=?1)",
            params![pattern_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    if !exists {
        return Err("定石が見つかりません".into());
    }
    let problem_count = conn
        .query_row(
            "SELECT COUNT(*) FROM problem_patterns WHERE pattern_id=?1",
            params![pattern_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    let related_pattern_count = conn
        .query_row(
            "SELECT COUNT(*) FROM pattern_relations
             WHERE from_pattern_id=?1 OR to_pattern_id=?1",
            params![pattern_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    Ok(PatternDeleteImpact {
        pattern_id,
        problem_count,
        related_pattern_count,
    })
}

pub fn delete_pattern(state: &AppState, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let changed = conn
        .execute("DELETE FROM patterns WHERE id=?1", params![id])
        .map_err(err_str)?;
    if changed == 0 {
        return Err("定石が見つかりません".into());
    }
    Ok(())
}

pub fn search_patterns(
    state: &AppState,
    query: PatternSearchQuery,
) -> Result<Vec<PatternSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let text = query.text.trim();
    let like = format!("%{}%", text);
    let text_empty = text.is_empty();
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let mut stmt = conn
        .prepare(
            "SELECT p.id,p.uuid,p.title,p.summary,p.pattern_type,p.updated_at,p.version,
                    (SELECT COUNT(*) FROM pattern_strategies s WHERE s.pattern_id=p.id),
                    (SELECT COUNT(*) FROM problem_patterns pp WHERE pp.pattern_id=p.id)
             FROM patterns p
             WHERE (?1 OR p.title LIKE ?2 OR p.summary LIKE ?2 OR p.situation LIKE ?2
                    OR p.principle LIKE ?2 OR p.cautions LIKE ?2 OR p.examples LIKE ?2
                    OR p.source_note LIKE ?2
                    OR EXISTS(SELECT 1 FROM pattern_strategies s WHERE s.pattern_id=p.id AND
                         (s.title LIKE ?2 OR s.description LIKE ?2 OR s.condition_text LIKE ?2 OR s.reasoning LIKE ?2))
                    OR EXISTS(SELECT 1 FROM pattern_tags t WHERE t.pattern_id=p.id AND t.tag LIKE ?2)
                    OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.value LIKE ?2))
               AND (?3 IS NULL OR p.pattern_type=?3)
               AND (?4 IS NULL OR EXISTS(SELECT 1 FROM pattern_tags t WHERE t.pattern_id=p.id AND t.tag=?4))
               AND (?5 IS NULL OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.facet_type='domain' AND f.value=?5))
               AND (?6 IS NULL OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.facet_type='goal' AND f.value=?6))
               AND (?7 IS NULL OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.facet_type='operation' AND f.value=?7))
               AND (?8 IS NULL OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.facet_type='structure' AND f.value=?8))
               AND (?9 IS NULL OR EXISTS(SELECT 1 FROM pattern_facets f WHERE f.pattern_id=p.id AND f.facet_type='situation' AND f.value=?9))
               AND (?10 IS NULL OR p.id<>?10)
             ORDER BY p.updated_at DESC,p.id DESC LIMIT ?11 OFFSET ?12",
        )
        .map_err(err_str)?;
    let mut rows: Vec<PatternSummary> = stmt
        .query_map(
            params![
                text_empty,
                like,
                query.pattern_type,
                query.tag,
                query.domain,
                query.goal,
                query.operation,
                query.structure,
                query.situation,
                query.exclude_id,
                limit,
                offset
            ],
            |row| {
                Ok(PatternSummary {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    pattern_type: row.get(4)?,
                    tags: vec![],
                    facets: PatternFacets::default(),
                    updated_at: row.get(5)?,
                    version: row.get(6)?,
                    strategy_count: row.get(7)?,
                    problem_count: row.get(8)?,
                })
            },
        )
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    for pattern in &mut rows {
        pattern.tags = tags_of(&conn, pattern.id).map_err(err_str)?;
        pattern.facets = facets_of(&conn, pattern.id).map_err(err_str)?;
    }
    Ok(rows)
}

fn distinct_values(conn: &Connection, sql: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare(sql).map_err(err_str)?;
    let rows = stmt
        .query_map([], |row| row.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str);
    rows
}

pub fn list_pattern_filter_values(state: &AppState) -> Result<PatternFilterValues, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    Ok(PatternFilterValues {
        pattern_types: distinct_values(
            &conn,
            "SELECT DISTINCT pattern_type FROM patterns WHERE pattern_type<>'' ORDER BY pattern_type",
        )?,
        tags: distinct_values(
            &conn,
            "SELECT DISTINCT tag FROM pattern_tags ORDER BY tag COLLATE NOCASE",
        )?,
        domains: facet_values_all(&conn, "domain")?,
        goals: facet_values_all(&conn, "goal")?,
        operations: facet_values_all(&conn, "operation")?,
        structures: facet_values_all(&conn, "structure")?,
        situations: facet_values_all(&conn, "situation")?,
    })
}

fn facet_values_all(conn: &Connection, facet_type: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT value FROM pattern_facets
             WHERE facet_type=?1 ORDER BY value COLLATE NOCASE",
        )
        .map_err(err_str)?;
    let rows = stmt
        .query_map(params![facet_type], |row| row.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str);
    rows
}

pub fn list_patterns_for_problem(
    state: &AppState,
    problem_id: i64,
) -> Result<Vec<ProblemPatternView>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT p.id,p.title,p.summary,p.pattern_type,pp.relation_type
             FROM problem_patterns pp JOIN patterns p ON p.id=pp.pattern_id
             WHERE pp.problem_id=?1 ORDER BY pp.relation_type,p.title,p.id",
        )
        .map_err(err_str)?;
    let mut rows: Vec<ProblemPatternView> = stmt
        .query_map(params![problem_id], |row| {
            Ok(ProblemPatternView {
                pattern_id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                pattern_type: row.get(3)?,
                relation_type: row.get(4)?,
                tags: vec![],
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    for row in &mut rows {
        row.tags = tags_of(&conn, row.pattern_id).map_err(err_str)?;
    }
    Ok(rows)
}

pub fn link_problem_pattern(
    state: &AppState,
    problem_id: i64,
    pattern_id: i64,
    relation_type: String,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute(
        "INSERT INTO problem_patterns(problem_id,pattern_id,relation_type,created_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(problem_id,pattern_id) DO UPDATE SET relation_type=excluded.relation_type",
        params![
            problem_id,
            pattern_id,
            normalized_relation(&relation_type, "applicable"),
            now_str()
        ],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn unlink_problem_pattern(
    state: &AppState,
    problem_id: i64,
    pattern_id: i64,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute(
        "DELETE FROM problem_patterns WHERE problem_id=?1 AND pattern_id=?2",
        params![problem_id, pattern_id],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn link_pattern_relation(
    state: &AppState,
    from_pattern_id: i64,
    to_pattern_id: i64,
    relation_type: String,
) -> Result<(), String> {
    if from_pattern_id == to_pattern_id {
        return Err("同じ定石自身は関連付けできません".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute(
        "INSERT OR IGNORE INTO pattern_relations
         (from_pattern_id,to_pattern_id,relation_type,created_at) VALUES (?1,?2,?3,?4)",
        params![
            from_pattern_id,
            to_pattern_id,
            normalized_relation(&relation_type, "related"),
            now_str()
        ],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn unlink_pattern_relation(
    state: &AppState,
    from_pattern_id: i64,
    to_pattern_id: i64,
    relation_type: String,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute(
        "DELETE FROM pattern_relations
         WHERE from_pattern_id=?1 AND to_pattern_id=?2 AND relation_type=?3",
        params![from_pattern_id, to_pattern_id, relation_type],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn list_pattern_versions(
    state: &AppState,
    pattern_id: i64,
) -> Result<Vec<PatternVersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let rows: Vec<(i64, i64, i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id,pattern_id,version,snapshot_json,saved_at FROM pattern_versions
                 WHERE pattern_id=?1 ORDER BY id DESC",
            )
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![pattern_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    rows.into_iter()
        .map(|(id, pattern_id, version, json, saved_at)| {
            let snapshot: PatternSnapshot = serde_json::from_str(&json).map_err(err_str)?;
            Ok(PatternVersionSummary {
                id,
                pattern_id,
                title: snapshot.title,
                version,
                saved_at,
            })
        })
        .collect()
}

pub fn get_pattern_version(
    state: &AppState,
    version_id: i64,
) -> Result<PatternVersionFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (id, pattern_id, version, json, saved_at): (i64, i64, i64, String, String) = conn
        .query_row(
            "SELECT id,pattern_id,version,snapshot_json,saved_at
             FROM pattern_versions WHERE id=?1",
            params![version_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(err_str)?;
    Ok(PatternVersionFull {
        id,
        pattern_id,
        version,
        saved_at,
        snapshot: serde_json::from_str(&json).map_err(err_str)?,
    })
}

pub fn restore_pattern_version(
    state: &AppState,
    version_id: i64,
    expected_version: Option<i64>,
) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let (pattern_id, json): (i64, String) = tx
        .query_row(
            "SELECT pattern_id,snapshot_json FROM pattern_versions WHERE id=?1",
            params![version_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(err_str)?;
    let current: i64 = tx
        .query_row(
            "SELECT version FROM patterns WHERE id=?1",
            params![pattern_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    if expected_version.is_some_and(|expected| expected != current) {
        return Err(format!("CONFLICT:{}", current));
    }
    let snapshot: PatternSnapshot = serde_json::from_str(&json).map_err(err_str)?;
    save_version(&tx, pattern_id)?;
    tx.execute(
        "UPDATE patterns SET title=?1,summary=?2,pattern_type=?3,situation=?4,principle=?5,
                cautions=?6,examples=?7,source_note=?8,updated_at=?9,version=version+1
         WHERE id=?10",
        params![
            snapshot.title,
            snapshot.summary,
            snapshot.pattern_type,
            snapshot.situation,
            snapshot.principle,
            snapshot.cautions,
            snapshot.examples,
            snapshot.source_note,
            now_str(),
            pattern_id
        ],
    )
    .map_err(err_str)?;
    set_tags(&tx, pattern_id, &snapshot.tags)?;
    set_facets(&tx, pattern_id, &snapshot.facets)?;
    set_strategies(&tx, pattern_id, &snapshot.strategies)?;
    tx.commit().map_err(err_str)?;
    Ok(current + 1)
}

#[derive(Serialize, Deserialize)]
struct PatternExportRelation {
    target_uuid: String,
    relation_type: String,
}

#[derive(Serialize, Deserialize)]
struct PatternExportProblemRelation {
    problem_id: i64,
    problem_title: String,
    relation_type: String,
}

#[derive(Serialize, Deserialize)]
struct PatternExportEntry {
    pattern: PatternSnapshot,
    #[serde(default)]
    related_patterns: Vec<PatternExportRelation>,
    #[serde(default)]
    related_problems: Vec<PatternExportProblemRelation>,
}

#[derive(Serialize, Deserialize)]
struct PatternLibraryExport {
    format: String,
    format_version: i64,
    patterns: Vec<PatternExportEntry>,
}

fn build_export(conn: &Connection, pattern_ids: Option<Vec<i64>>) -> Result<String, String> {
    let ids = if let Some(ids) = pattern_ids {
        ids
    } else {
        let mut stmt = conn
            .prepare("SELECT id FROM patterns ORDER BY id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let mut patterns = Vec::new();
    for id in ids {
        let pattern = snapshot_of(conn, id)?;
        let related_patterns = {
            let mut stmt = conn
                .prepare(
                    "SELECT p.uuid,r.relation_type FROM pattern_relations r
                     JOIN patterns p ON p.id=r.to_pattern_id WHERE r.from_pattern_id=?1
                     ORDER BY r.relation_type,p.uuid",
                )
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![id], |row| {
                    Ok(PatternExportRelation {
                        target_uuid: row.get(0)?,
                        relation_type: row.get(1)?,
                    })
                })
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        };
        let related_problems = {
            let mut stmt = conn
                .prepare(
                    "SELECT p.id,p.title,pp.relation_type FROM problem_patterns pp
                     JOIN problems p ON p.id=pp.problem_id WHERE pp.pattern_id=?1
                     ORDER BY p.id",
                )
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![id], |row| {
                    Ok(PatternExportProblemRelation {
                        problem_id: row.get(0)?,
                        problem_title: row.get(1)?,
                        relation_type: row.get(2)?,
                    })
                })
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        };
        patterns.push(PatternExportEntry {
            pattern,
            related_patterns,
            related_problems,
        });
    }
    serde_json::to_string_pretty(&PatternLibraryExport {
        format: "kyozai-kobo-pattern-library".into(),
        format_version: 1,
        patterns,
    })
    .map_err(err_str)
}

pub fn export_patterns_json(
    state: &AppState,
    pattern_ids: Option<Vec<i64>>,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    build_export(&conn, pattern_ids)
}

pub fn export_patterns_file(
    state: &AppState,
    pattern_ids: Option<Vec<i64>>,
    dest_path: String,
) -> Result<(), String> {
    let json = export_patterns_json(state, pattern_ids)?;
    std::fs::write(dest_path, json).map_err(err_str)
}

pub fn import_patterns_json(
    state: &AppState,
    json_text: String,
) -> Result<ImportPatternsResult, String> {
    let data: PatternLibraryExport = serde_json::from_str(&json_text)
        .map_err(|error| format!("定石JSONを読み込めません: {}", error))?;
    if data.format != "kyozai-kobo-pattern-library" || data.format_version != 1 {
        return Err("対応していない定石ライブラリ形式です".into());
    }
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let mut ids = HashMap::new();
    let mut created = 0;
    let mut skipped = 0;
    for entry in &data.patterns {
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM patterns WHERE uuid=?1",
                params![entry.pattern.uuid],
                |row| row.get(0),
            )
            .optional()
            .map_err(err_str)?;
        if let Some(id) = existing {
            ids.insert(entry.pattern.uuid.clone(), id);
            skipped += 1;
            continue;
        }
        let now = now_str();
        tx.execute(
            "INSERT INTO patterns(uuid,title,summary,pattern_type,situation,principle,cautions,examples,
                                  source_note,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![
                entry.pattern.uuid,
                entry.pattern.title,
                entry.pattern.summary,
                normalized_type(&entry.pattern.pattern_type),
                entry.pattern.situation,
                entry.pattern.principle,
                entry.pattern.cautions,
                entry.pattern.examples,
                entry.pattern.source_note,
                now
            ],
        )
        .map_err(err_str)?;
        let id = tx.last_insert_rowid();
        set_tags(&tx, id, &entry.pattern.tags)?;
        set_facets(&tx, id, &entry.pattern.facets)?;
        let strategies: Vec<_> = entry
            .pattern
            .strategies
            .iter()
            .cloned()
            .map(|mut strategy| {
                strategy.id = None;
                strategy.parent_strategy_id = None;
                strategy
            })
            .collect();
        set_strategies(&tx, id, &strategies)?;
        ids.insert(entry.pattern.uuid.clone(), id);
        created += 1;
    }
    let mut relations_created = 0;
    let mut problem_relations_created = 0;
    for entry in &data.patterns {
        let Some(&from_id) = ids.get(&entry.pattern.uuid) else {
            continue;
        };
        for relation in &entry.related_patterns {
            let target_id = if let Some(id) = ids.get(&relation.target_uuid).copied() {
                Some(id)
            } else {
                tx.query_row(
                    "SELECT id FROM patterns WHERE uuid=?1",
                    params![relation.target_uuid],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err_str)?
            };
            if let Some(target_id) = target_id {
                if from_id != target_id {
                    relations_created += tx
                        .execute(
                            "INSERT OR IGNORE INTO pattern_relations
                             (from_pattern_id,to_pattern_id,relation_type,created_at)
                             VALUES (?1,?2,?3,?4)",
                            params![from_id, target_id, relation.relation_type, now_str()],
                        )
                        .map_err(err_str)? as i64;
                }
            }
        }
        for relation in &entry.related_problems {
            let title: Option<String> = tx
                .query_row(
                    "SELECT title FROM problems WHERE id=?1",
                    params![relation.problem_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(err_str)?;
            if title.as_deref() == Some(relation.problem_title.as_str()) {
                problem_relations_created += tx
                    .execute(
                        "INSERT OR IGNORE INTO problem_patterns
                         (problem_id,pattern_id,relation_type,created_at) VALUES (?1,?2,?3,?4)",
                        params![
                            relation.problem_id,
                            from_id,
                            relation.relation_type,
                            now_str()
                        ],
                    )
                    .map_err(err_str)? as i64;
            }
        }
    }
    tx.commit().map_err(err_str)?;
    Ok(ImportPatternsResult {
        created,
        skipped,
        relations_created,
        problem_relations_created,
    })
}

pub fn import_patterns_file(
    state: &AppState,
    path: String,
) -> Result<ImportPatternsResult, String> {
    let json = std::fs::read_to_string(path).map_err(err_str)?;
    import_patterns_json(state, json)
}
