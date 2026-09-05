//! 「考え方」の保存・描画。
//!
//! 構造化Blockを正本とし、既存の `explanation_latex` にはここで描画した
//! 互換LaTeXを保存する。Pattern本文はBlock内のsnapshotから既存rendererへ渡す。

use crate::models::{ProblemSolutionVariant, SolutionFlowBlock};
use crate::state::AppState;
use std::collections::HashSet;

const MAX_FLOW_BLOCKS: usize = 120;

fn strip_solution_heading_wrapper(line: &str) -> String {
    let mut value = line.trim();
    value = value.trim_end_matches("\\par").trim_end();
    value = value.trim_end_matches("\\\\").trim_end();
    if let Some(rest) = value.strip_prefix("\\noindent") {
        value = rest.trim_start();
    }
    if let Some(inner) = value
        .strip_prefix("\\textbf{")
        .and_then(|rest| rest.strip_suffix('}'))
    {
        value = inner.trim();
    }
    value.trim().to_string()
}

fn without_leading_part_number(value: &str) -> &str {
    let trimmed = value.trim_start();
    for (opening, closing) in [('(', ')'), ('（', '）')] {
        if let Some(rest) = trimmed.strip_prefix(opening) {
            if let Some(end) = rest.find(closing) {
                let number = &rest[..end];
                if !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
                {
                    return rest[end + closing.len_utf8()..].trim_start();
                }
            }
        }
    }
    trimmed
}

fn is_solution_method_heading(value: &str) -> bool {
    let core = without_leading_part_number(value);
    core.starts_with("解法")
        || core.starts_with("別解")
        || core.starts_with("【解法")
        || core.starts_with("【別解")
}

fn is_standalone_part_heading(value: &str) -> bool {
    let trimmed = value.trim();
    for (opening, closing) in [('(', ')'), ('（', '）')] {
        let Some(rest) = trimmed.strip_prefix(opening) else {
            continue;
        };
        let Some(end) = rest.find(closing) else {
            continue;
        };
        let number = &rest[..end];
        if !number.is_empty()
            && number.chars().all(|character| character.is_ascii_digit())
            && rest[end + closing.len_utf8()..].trim().is_empty()
        {
            return true;
        }
    }
    false
}

fn strip_leading_part_marker<'a>(value: &'a str, expected: &str) -> Option<&'a str> {
    let trimmed = value.trim_start();
    let rest = trimmed.strip_prefix(expected)?.trim_start();
    Some(
        rest.strip_prefix("では")
            .unwrap_or(rest)
            .trim_start_matches(['、', ',', ' ']),
    )
}

/// 既存答案に明記された解法見出しだけを、表示文字列のまま取り出す。
pub fn extract_solution_method_headings(solution: &str) -> Vec<String> {
    solution
        .lines()
        .map(strip_solution_heading_wrapper)
        .filter(|line| !line.is_empty() && line.chars().count() <= 160)
        .filter(|line| is_solution_method_heading(line))
        .collect()
}

/// 答案で独立している「(2)」等の設問見出しを表示文字列のまま取り出す。
pub fn extract_solution_part_headings(solution: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    solution
        .lines()
        .map(strip_solution_heading_wrapper)
        .filter(|line| is_standalone_part_heading(line) && seen.insert(line.clone()))
        .collect()
}

/// 「(2)では、…」と本文へ埋もれた設問番号を、答案と同じ独立Headingへ直す。
pub fn promote_flow_part_headings(
    blocks: &mut Vec<SolutionFlowBlock>,
    answer_part_headings: &[String],
) -> Result<(), String> {
    for expected in answer_part_headings {
        if blocks
            .iter()
            .any(|block| block.block_type == "heading" && block.text.trim() == expected.trim())
        {
            continue;
        }

        let Some((index, source_field, remainder)) =
            blocks.iter().enumerate().find_map(|(index, block)| {
                let (source_field, source) = match block.block_type.as_str() {
                    "text" => ("content", block.content.as_str()),
                    "heading" => ("text", block.text.as_str()),
                    _ => return None,
                };
                strip_leading_part_marker(source, expected)
                    .map(|remainder| (index, source_field, remainder.trim().to_string()))
            })
        else {
            return Err(format!(
                "考え方に答案の設問見出し「{}」がありません。設問が切り替わる位置へ独立したheading Blockとして入れてください",
                expected.trim()
            ));
        };

        let heading = SolutionFlowBlock {
            id: format!("{}-part-heading", blocks[index].id.trim()),
            block_type: "heading".into(),
            text: expected.trim().to_string(),
            ..Default::default()
        };
        let remainder_is_empty = remainder.is_empty();
        if source_field == "content" {
            blocks[index].content = remainder;
        } else {
            blocks[index].text = remainder;
        }
        if remainder_is_empty {
            blocks[index] = heading;
        } else {
            blocks.insert(index, heading);
        }
    }
    Ok(())
}

/// 1枚の定石に複数の使用Candidateがあり、その直後で複数解法へ分かれる場合、
/// 定石カードを最初の解法見出しより前へ移す。各Candidateの選択理由は各枝側に残す。
pub fn hoist_branching_pattern_before_method_headings(blocks: &mut Vec<SolutionFlowBlock>) {
    let method_heading_indexes: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            (block.block_type == "heading" && is_solution_method_heading(&block.text))
                .then_some(index)
        })
        .collect();
    if method_heading_indexes.len() < 2 {
        return;
    }
    let first_heading = method_heading_indexes[0];
    let second_heading = method_heading_indexes[1];
    let indexes_to_move: Vec<usize> = (first_heading + 1..second_heading)
        .filter(|index| {
            let block = &blocks[*index];
            block.block_type == "pattern" && block.used_strategy_ids.len() >= 2
        })
        .collect();
    if indexes_to_move.is_empty() {
        return;
    }

    let mut moved = Vec::with_capacity(indexes_to_move.len());
    for index in indexes_to_move.into_iter().rev() {
        moved.push(blocks.remove(index));
    }
    moved.reverse();
    let first_heading = blocks
        .iter()
        .position(|block| block.block_type == "heading" && is_solution_method_heading(&block.text))
        .unwrap_or(0);
    blocks.splice(first_heading..first_heading, moved);
}

/// AIが見出しを教育的な別表現へ要約しても、既存答案の見出しへ戻す。
pub fn align_flow_headings_with_answer(
    blocks: &mut [SolutionFlowBlock],
    answer_headings: &[String],
) -> Result<(), String> {
    if answer_headings.is_empty() {
        return Ok(());
    }
    let heading_indexes: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            (block.block_type == "heading" && is_solution_method_heading(&block.text))
                .then_some(index)
        })
        .collect();
    if heading_indexes.len() != answer_headings.len() {
        return Err(format!(
            "考え方の解法見出しが既存答案と一致しません（答案{}件、Flow{}件）。答案の見出しを省略・要約せず、そのままHeading Blockへ入れてください",
            answer_headings.len(),
            heading_indexes.len()
        ));
    }
    for (index, answer_heading) in heading_indexes.into_iter().zip(answer_headings) {
        blocks[index].text = answer_heading.clone();
    }
    Ok(())
}

fn non_empty(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label}が空です"))
    } else if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        Err(format!(
            "{label}にLaTeXで扱えない制御文字が含まれています。考え方を再生成してください"
        ))
    } else if value.chars().count() > 50_000 {
        Err(format!("{label}が長すぎます"))
    } else {
        Ok(())
    }
}

/// 保存済みsnapshotを勝手に最新版へ変えず、形と参照だけを検証する。
pub fn normalize_saved_flow_blocks(
    mut blocks: Vec<SolutionFlowBlock>,
    inherited_pattern_ids: &HashSet<i64>,
) -> Result<Vec<SolutionFlowBlock>, String> {
    if blocks.len() > MAX_FLOW_BLOCKS {
        return Err(format!("考え方は最大{MAX_FLOW_BLOCKS}ブロックです"));
    }
    let mut seen_ids = HashSet::new();
    // 同じ定石を別の設問や後段の判断で再利用する場合は、必要な位置へもう一度
    // 引用できる。共通部分の直後や隣接位置に同じカードを重ねるだけの重複は落とす。
    let mut has_new_context = false;
    let mut last_pattern_id = None;
    let mut normalized = Vec::with_capacity(blocks.len());
    for (index, mut block) in blocks.drain(..).enumerate() {
        block.id = block.id.trim().chars().take(100).collect();
        if block.id.is_empty() || !seen_ids.insert(block.id.clone()) {
            block.id = format!("flow-block-{}", index + 1);
            while !seen_ids.insert(block.id.clone()) {
                block.id.push('x');
            }
        }
        match block.block_type.as_str() {
            // Editorでは追加直後の空Blockも保持する。保存時は未入力Blockだけを
            // 落とし、入力途中のために問題全体の保存を失敗させない。
            "text" | "caution" if block.content.trim().is_empty() => continue,
            "formula" if block.latex.trim().is_empty() => continue,
            "heading" if block.text.trim().is_empty() => continue,
            "text" | "caution" => non_empty(&block.content, "考え方の文章")?,
            "formula" => non_empty(&block.latex, "考え方の数式")?,
            "heading" => non_empty(&block.text, "考え方の見出し")?,
            "pattern" => {
                let pattern_id = block
                    .pattern_id
                    .filter(|id| *id > 0)
                    .ok_or("定石BlockにpatternIdがありません")?;
                let repeats_common_without_new_context =
                    inherited_pattern_ids.contains(&pattern_id) && !has_new_context;
                let is_adjacent_duplicate = last_pattern_id == Some(pattern_id);
                if repeats_common_without_new_context || is_adjacent_duplicate {
                    continue;
                }
                let snapshot = block
                    .snapshot
                    .as_ref()
                    .ok_or("定石BlockにPatternSnapshotがありません")?;
                let version = block.pattern_version.unwrap_or(snapshot.version);
                if version < 0 || snapshot.version != version {
                    return Err("定石BlockのpatternVersionとsnapshotが一致しません".into());
                }
                let available: HashSet<i64> = snapshot
                    .strategies
                    .iter()
                    .filter_map(|strategy| strategy.id)
                    .collect();
                block.used_strategy_ids.sort_unstable();
                block.used_strategy_ids.dedup();
                if block
                    .used_strategy_ids
                    .iter()
                    .any(|id| !available.contains(id))
                {
                    return Err("定石Blockに存在しないCandidate Strategyが指定されています".into());
                }
                block.pattern_version = Some(version);
                last_pattern_id = Some(pattern_id);
            }
            _ => return Err(format!("未対応の考え方Blockです: {}", block.block_type)),
        }
        if block.block_type != "pattern" {
            has_new_context = true;
            last_pattern_id = None;
        }
        normalized.push(block);
    }
    Ok(normalized)
}

/// AIはPattern IDとCandidate IDだけを返す。ここでcanonical DBからsnapshotを取得する。
pub fn hydrate_ai_flow_blocks(
    state: &AppState,
    mut blocks: Vec<SolutionFlowBlock>,
    inherited_pattern_ids: &HashSet<i64>,
) -> Result<Vec<SolutionFlowBlock>, String> {
    let mut hydrated = Vec::with_capacity(blocks.len());
    for mut block in blocks.drain(..) {
        normalize_generated_flow_for_students(&mut block)?;
        if block.block_type == "pattern" {
            let pattern_id = block
                .pattern_id
                .filter(|id| *id > 0)
                .ok_or("AIが定石Blockへ有効なpatternIdを指定しませんでした")?;
            let snapshot = super::patterns::pattern_snapshot(state, pattern_id)
                .map_err(|_| format!("AIが存在しないpatternId {pattern_id} を返しました"))?;
            block.pattern_version = Some(snapshot.version);
            block.snapshot = Some(snapshot);
        }
        hydrated.push(block);
    }
    let normalized = normalize_saved_flow_blocks(hydrated, inherited_pattern_ids)?;
    normalize_saved_flow_blocks(
        ensure_pattern_usage_explanations(normalized),
        inherited_pattern_ids,
    )
}

/// AIが大学数学寄りの短い専門語を混ぜても、そのまま教材へ保存しない。
/// 数学的内容を変えずに言い換えられる語だけをここで補正する。
pub fn normalize_generated_flow_for_students(block: &mut SolutionFlowBlock) -> Result<(), String> {
    fn replace_terms(value: &mut String) {
        *value = value
            .replace("既存答案の結論に至る", "求める結論が得られる")
            .replace("保存済み答案の結論に至る", "求める結論が得られる")
            .replace("既存の答案の結論に至る", "求める結論が得られる")
            .replace("保存済みの答案の結論に至る", "求める結論が得られる")
            .replace("既存答案の置換", "この置換")
            .replace("保存済み答案の置換", "この置換")
            .replace("既存の答案の置換", "この置換")
            .replace("保存済みの答案の置換", "この置換")
            .replace("保存済みの既存答案", "この解法")
            .replace("保存済みの答案", "この解法")
            .replace("保存済み答案", "この解法")
            .replace("既存の答案", "この解法")
            .replace("既存答案", "この解法")
            .replace("既存の解答", "この解法")
            .replace("保存済みの解答", "この解法")
            .replace("参照答案", "この解法")
            .replace("参照解答", "この解法")
            .replace("元の答案", "この解法")
            .replace("元の解答", "この解法")
            .replace("クロスターム", "積を含む項")
            .replace("交差項", "積を含む項")
            .replace("下界", "これより小さくならない値")
            .replace("上界", "これより大きくならない値")
            .replace("Candidate", "候補")
            .replace("Pattern", "定石")
            .replace("Flow", "考え方")
            .replace("Block", "部分")
            .replace("Snapshot", "保存時点の定石");
    }

    replace_terms(&mut block.content);
    replace_terms(&mut block.text);
    replace_terms(&mut block.latex);

    if matches!(block.block_type.as_str(), "text" | "caution") {
        for term in ["答案", "保存済み", "生成元", "プロンプト", "AI", "ジョブ"] {
            if block.content.contains(term) {
                return Err(format!(
                    "考え方の教材本文に制作過程を示す語「{term}」を入れず、数学上の判断を直接説明してください"
                ));
            }
        }
    }
    Ok(())
}

fn candidate_action_name(title: &str) -> &str {
    title
        .trim()
        .strip_suffix("の利用")
        .or_else(|| title.trim().strip_suffix("の活用"))
        .unwrap_or_else(|| title.trim())
}

fn default_pattern_usage_explanation(block: &SolutionFlowBlock) -> String {
    let Some(snapshot) = block.snapshot.as_ref() else {
        return "ここでは、この定石を用いる。".into();
    };
    let used: Vec<&str> = block
        .used_strategy_ids
        .iter()
        .filter_map(|strategy_id| {
            snapshot
                .strategies
                .iter()
                .find(|strategy| strategy.id == Some(*strategy_id))
                .map(|strategy| candidate_action_name(&strategy.title))
        })
        .filter(|title| !title.is_empty())
        .collect();
    match used.as_slice() {
        [] => "ここでは、この定石を用いる。".into(),
        [title] => format!("ここでは、{title}を用いる。"),
        titles => format!(
            "ここでは、{}のいずれも使えるため、それぞれを用いる解法を考える。",
            titles.join("と")
        ),
    }
}

/// 定石カードの内部へ使用ラベルを書かず、直後の編集可能な文章Blockで
/// この問題における選択を説明する。AIが理由を書いた場合はその文章を保ち、
/// 書き忘れた場合だけCandidate情報から最低限の一文を補う。
fn ensure_pattern_usage_explanations(blocks: Vec<SolutionFlowBlock>) -> Vec<SolutionFlowBlock> {
    let mut completed = Vec::with_capacity(blocks.len());
    let mut blocks = blocks.into_iter().peekable();
    while let Some(block) = blocks.next() {
        let needs_explanation = block.block_type == "pattern"
            && !blocks.peek().is_some_and(|next| {
                next.block_type == "text" && next.content.trim_start().starts_with("ここでは")
            });
        let explanation = needs_explanation.then(|| SolutionFlowBlock {
            id: format!(
                "{}-usage",
                if block.id.trim().is_empty() {
                    "pattern"
                } else {
                    block.id.trim()
                }
            ),
            block_type: "text".into(),
            content: default_pattern_usage_explanation(&block),
            ..Default::default()
        });
        completed.push(block);
        if let Some(explanation) = explanation {
            completed.push(explanation);
        }
    }
    completed
}

fn display_formula(latex: &str) -> String {
    let trimmed = latex.trim();
    let inline_dollar = (!trimmed.starts_with("$$"))
        .then(|| {
            trimmed
                .strip_prefix('$')
                .and_then(|rest| rest.strip_suffix('$'))
        })
        .flatten();
    let standalone_environment = [
        "align",
        "align*",
        "alignat",
        "alignat*",
        "equation",
        "equation*",
        "flalign",
        "flalign*",
        "gather",
        "gather*",
        "multline",
        "multline*",
        "displaymath",
    ]
    .iter()
    .any(|environment| trimmed.starts_with(&format!("\\begin{{{environment}}}")));
    if standalone_environment {
        format!("{trimmed}\n")
    } else if let Some(inner) = trimmed
        .strip_prefix("\\[")
        .and_then(|rest| rest.strip_suffix("\\]"))
    {
        display_formula_inner(inner)
    } else if let Some(inner) = trimmed
        .strip_prefix("$$")
        .and_then(|rest| rest.strip_suffix("$$"))
    {
        display_formula_inner(inner)
    } else if let Some(inner) = trimmed
        .strip_prefix("\\(")
        .and_then(|rest| rest.strip_suffix("\\)"))
    {
        display_formula_inner(inner)
    } else if let Some(inner) = inline_dollar {
        display_formula_inner(inner)
    } else {
        display_formula_inner(trimmed)
    }
}

/// AI生成前の古いFlowにも、狭い列で横へ並びすぎる条件列挙が残っている。
/// \qquad で並べただけの独立した条件は、意味を変えず縦へ積んで表示する。
fn compact_plain_formula_for_columns(latex: &str) -> String {
    let trimmed = latex.trim();
    if trimmed.contains("\\begin{") || trimmed.contains("\\\\") {
        return trimmed.to_string();
    }
    let parts = trimmed
        .split("\\qquad")
        .map(|part| part.trim().trim_end_matches(',').trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let is_wide = trimmed
        .chars()
        .filter(|character| !character.is_whitespace())
        .count()
        > 80;
    if parts.len() < 2 || (parts.len() == 2 && !is_wide) {
        return trimmed.to_string();
    }
    format!(
        "\\begin{{gathered}}\n{}\n\\end{{gathered}}",
        parts.join("\\\\\n")
    )
}

fn display_formula_inner(latex: &str) -> String {
    let compact = compact_plain_formula_for_columns(latex);
    format!("\\[\n{compact}\n\\]\n")
}

pub fn render_flow_blocks(blocks: &[SolutionFlowBlock]) -> String {
    let mut out = String::new();
    for (index, block) in blocks.iter().enumerate() {
        let next_is_formula = blocks
            .get(index + 1)
            .is_some_and(|next| next.block_type == "formula");
        match block.block_type.as_str() {
            "text" => {
                out.push_str(block.content.trim());
                if next_is_formula {
                    // 表示数式は直前の文章と同じ段落内で開始する。ここで \par を
                    // 入れると空の行送りが数式の上側だけへ残り、上下が不均衡になる。
                    out.push('\n');
                } else {
                    out.push_str("\n\\par\n");
                }
            }
            "formula" => out.push_str(&display_formula(&block.latex)),
            "heading" => {
                out.push_str(&format!(
                    "\\par\\smallskip\n\\noindent\\textbf{{{}}}\\par\n",
                    block.text.trim()
                ));
            }
            "caution" => {
                out.push_str(&format!(
                    "\\par\\smallskip\n\\noindent\\textbf{{【注意】}} {}\\par\n",
                    block.content.trim()
                ));
            }
            "pattern" => {
                if let Some(snapshot) = block.snapshot.as_ref() {
                    out.push_str(&super::pattern_card::render_pattern_card_with_usage(
                        snapshot,
                        &block.used_strategy_ids,
                    ));
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    let rendered = out.trim();
    if rendered.is_empty() {
        String::new()
    } else {
        // Flowは判断と数式が交互に現れやすい。上下を同じ値にして、数式の
        // 前後が視覚的に片寄らないようにする。
        format!(
            "{{\\setlength{{\\abovedisplayskip}}{{0.4em}}%\n\\setlength{{\\belowdisplayskip}}{{0.4em}}%\n\\setlength{{\\abovedisplayshortskip}}{{0.25em}}%\n\\setlength{{\\belowdisplayshortskip}}{{0.25em}}%\n{rendered}\n}}"
        )
    }
}

fn ordered_variants(variants: &[ProblemSolutionVariant]) -> Vec<&ProblemSolutionVariant> {
    let mut ordered: Vec<_> = variants.iter().collect();
    ordered.sort_by_key(|variant| if variant.role == "main" { 0 } else { 1 });
    ordered
}

/// 共通Flow → 解法ごとのFlowの順に描画する。構造化Flowが1件もない旧データは
/// 従来のexplanationをそのまま互換表示する。
pub fn render_teaching_flow_latex(
    common_flow_blocks: &[SolutionFlowBlock],
    variants: &[ProblemSolutionVariant],
) -> String {
    let ordered = ordered_variants(variants);
    let has_structured_flow = !common_flow_blocks.is_empty()
        || ordered
            .iter()
            .any(|variant| !variant.flow_blocks.is_empty());
    if !has_structured_flow {
        return ordered
            .iter()
            .filter_map(|variant| {
                variant
                    .explanation
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
            })
            .enumerate()
            .map(|(index, text)| {
                if index == 0 {
                    text.trim().to_string()
                } else {
                    format!("【別解{index}】\n{}", text.trim())
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }

    let mut sections = Vec::new();
    let common = render_flow_blocks(common_flow_blocks);
    if !common.is_empty() {
        sections.push(common);
    }
    let visible: Vec<_> = ordered
        .into_iter()
        .filter(|variant| !variant.flow_blocks.is_empty())
        .collect();
    let show_branch_heading = visible.len() > 1 || !common_flow_blocks.is_empty();
    let mut alternative_index = 0;
    for (index, variant) in visible.iter().enumerate() {
        let flow = render_flow_blocks(&variant.flow_blocks);
        if flow.is_empty() {
            continue;
        }
        if show_branch_heading {
            let label = if variant.role == "main" {
                "【解法1】".to_string()
            } else {
                alternative_index += 1;
                format!("【別解{}】", alternative_index)
            };
            sections.push(format!(
                "\\par\\medskip\n\\noindent\\textbf{{{label}}}\\par\n{flow}"
            ));
        } else if index == 0 {
            sections.push(flow);
        }
    }
    sections.join("\n\n")
}

pub fn render_solution_flow_latex(
    common_flow_blocks: Vec<SolutionFlowBlock>,
    solution_variants: Vec<ProblemSolutionVariant>,
) -> Result<String, String> {
    let common = normalize_saved_flow_blocks(common_flow_blocks, &HashSet::new())?;
    let common_pattern_ids: HashSet<i64> =
        common.iter().filter_map(|block| block.pattern_id).collect();
    let mut variants = solution_variants;
    for variant in &mut variants {
        variant.flow_blocks = normalize_saved_flow_blocks(
            std::mem::take(&mut variant.flow_blocks),
            &common_pattern_ids,
        )?;
    }
    Ok(render_teaching_flow_latex(&common, &variants))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PatternFacets, PatternSnapshot, PatternStrategyInput, SolutionStrategy};

    fn snapshot(version: i64) -> PatternSnapshot {
        PatternSnapshot {
            version,
            uuid: "pattern-uuid".into(),
            title: "関数値の差 \\(f(b)-f(a)\\) の扱い".into(),
            summary: String::new(),
            pattern_type: "strategy".into(),
            situation: String::new(),
            principle: String::new(),
            cautions: String::new(),
            examples: String::new(),
            source_note: String::new(),
            tags: vec![],
            facets: PatternFacets::default(),
            strategies: vec![
                PatternStrategyInput {
                    id: Some(101),
                    title: "平均値の定理の利用".into(),
                    sort_order: 1,
                    ..Default::default()
                },
                PatternStrategyInput {
                    id: Some(102),
                    title: "定積分表示の利用".into(),
                    sort_order: 2,
                    ..Default::default()
                },
            ],
        }
    }

    fn pattern_block(id: &str, used: Vec<i64>, version: i64) -> SolutionFlowBlock {
        SolutionFlowBlock {
            id: id.into(),
            block_type: "pattern".into(),
            pattern_id: Some(42),
            pattern_version: Some(version),
            snapshot: Some(snapshot(version)),
            used_strategy_ids: used,
            ..Default::default()
        }
    }

    fn variant(role: &str, flow_blocks: Vec<SolutionFlowBlock>) -> ProblemSolutionVariant {
        ProblemSolutionVariant {
            id: format!("variant-{role}"),
            role: role.into(),
            strategy: SolutionStrategy {
                id: format!("strategy-{role}"),
                title: if role == "main" {
                    "平均値の定理"
                } else {
                    "定積分"
                }
                .into(),
                summary: "完答する".into(),
                ..Default::default()
            },
            flow_blocks,
            ..Default::default()
        }
    }

    #[test]
    fn common_pattern_is_full_and_not_repeated_in_variants() {
        let common = vec![pattern_block("common-pattern", vec![101, 102], 3)];
        let variants = vec![
            variant("main", vec![pattern_block("duplicate", vec![101], 3)]),
            variant(
                "alternative",
                vec![SolutionFlowBlock {
                    id: "alt-text".into(),
                    block_type: "text".into(),
                    content: "上の定石の②を用いる。".into(),
                    ..Default::default()
                }],
            ),
        ];
        let latex = render_solution_flow_latex(common, variants).unwrap();
        assert_eq!(latex.matches("\\begin{tcolorbox}").count(), 1);
        assert!(latex.contains("平均値の定理の利用"));
        assert!(latex.contains("定積分表示の利用"));
        assert!(!latex.contains("今回使用"));
    }

    #[test]
    fn ai_pattern_block_gets_a_usage_sentence_after_the_card() {
        let blocks = ensure_pattern_usage_explanations(vec![
            pattern_block("pattern", vec![101], 3),
            SolutionFlowBlock {
                id: "formula".into(),
                block_type: "formula".into(),
                latex: "f(b)-f(a)".into(),
                ..Default::default()
            },
        ]);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].block_type, "text");
        assert_eq!(blocks[1].content, "ここでは、平均値の定理を用いる。");
    }

    #[test]
    fn ai_usage_reason_immediately_after_pattern_is_preserved() {
        let reason = SolutionFlowBlock {
            id: "reason".into(),
            block_type: "text".into(),
            content: "ここでは、関数値の差であるから平均値の定理を用いる。".into(),
            ..Default::default()
        };
        let blocks = ensure_pattern_usage_explanations(vec![
            pattern_block("pattern", vec![101], 3),
            reason.clone(),
        ]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].content, reason.content);
    }

    #[test]
    fn shared_candidate_pattern_is_placed_before_method_branches() {
        let mut blocks = vec![
            SolutionFlowBlock {
                id: "method-1".into(),
                block_type: "heading".into(),
                text: "(1) 解法1（成分表示を用いる方法）".into(),
                ..Default::default()
            },
            pattern_block("complex-pattern", vec![101, 102], 3),
            SolutionFlowBlock {
                id: "choice-1".into(),
                block_type: "text".into(),
                content: "ここでは、成分表示を用いる。".into(),
                ..Default::default()
            },
            SolutionFlowBlock {
                id: "method-2".into(),
                block_type: "heading".into(),
                text: "解法2（極形式を用いる方法）".into(),
                ..Default::default()
            },
            SolutionFlowBlock {
                id: "choice-2".into(),
                block_type: "text".into(),
                content: "ここでは、極形式を用いる。".into(),
                ..Default::default()
            },
        ];

        hoist_branching_pattern_before_method_headings(&mut blocks);
        let blocks = ensure_pattern_usage_explanations(blocks);

        assert_eq!(blocks[0].block_type, "pattern");
        assert_eq!(blocks[1].block_type, "text");
        assert!(blocks[1].content.contains("それぞれを用いる解法"));
        assert_eq!(blocks[2].text, "(1) 解法1（成分表示を用いる方法）");
        assert_eq!(blocks[3].content, "ここでは、成分表示を用いる。");
        assert_eq!(blocks[4].text, "解法2（極形式を用いる方法）");
        assert_eq!(blocks[5].content, "ここでは、極形式を用いる。");
    }

    #[test]
    fn embedded_part_number_is_promoted_to_a_bold_flow_heading() {
        let answer = "(1) 解法1（成分表示を用いる方法）\n本文\n\n(2)\n本文";
        let headings = extract_solution_part_headings(answer);
        assert_eq!(headings, vec!["(2)"]);
        let mut blocks = vec![SolutionFlowBlock {
            id: "part-2-text".into(),
            block_type: "text".into(),
            content: "(2)では、(1)を利用できる形へ直す。".into(),
            ..Default::default()
        }];

        promote_flow_part_headings(&mut blocks, &headings).unwrap();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].block_type, "heading");
        assert_eq!(blocks[0].text, "(2)");
        assert_eq!(blocks[1].content, "(1)を利用できる形へ直す。");
        let latex = render_flow_blocks(&blocks);
        assert!(latex.contains("\\noindent\\textbf{(2)}"));
    }

    #[test]
    fn ai_flow_rephrases_nonstandard_short_terms() {
        let mut block = SolutionFlowBlock {
            id: "wording".into(),
            block_type: "text".into(),
            content: "交差項を整理し、下界を求める。".into(),
            ..Default::default()
        };
        normalize_generated_flow_for_students(&mut block).unwrap();
        assert_eq!(
            block.content,
            "積を含む項を整理し、これより小さくならない値を求める。"
        );
    }

    #[test]
    fn ai_flow_removes_answer_production_language() {
        let mut block = SolutionFlowBlock {
            id: "wording".into(),
            block_type: "text".into(),
            content: "分数指数を消すため、既存答案の置換を用いる。計算すれば既存答案の結論に至る。"
                .into(),
            ..Default::default()
        };
        normalize_generated_flow_for_students(&mut block).unwrap();
        assert_eq!(
            block.content,
            "分数指数を消すため、この置換を用いる。計算すれば求める結論が得られる。"
        );
        assert!(!block.content.contains("答案"));
    }

    #[test]
    fn log_difference_branches_into_mean_value_and_integral_solutions() {
        let common = vec![
            SolutionFlowBlock {
                id: "recognize-difference".into(),
                block_type: "text".into(),
                content: r"\(\log(b/a)=\log b-\log a\) は関数値の差になっている。".into(),
                ..Default::default()
            },
            pattern_block("difference-pattern", vec![101, 102], 4),
        ];
        let mut main = variant(
            "main",
            vec![
                SolutionFlowBlock {
                    id: "mvt-choice".into(),
                    block_type: "text".into(),
                    content: "上の定石の①（平均値の定理）を用いる。".into(),
                    ..Default::default()
                },
                SolutionFlowBlock {
                    id: "mvt-formula".into(),
                    block_type: "formula".into(),
                    latex: r"\log b-\log a=\dfrac{b-a}{c}\quad(a<c<b)".into(),
                    ..Default::default()
                },
            ],
        );
        main.solution =
            r"\(a<c<b\) を満たす \(c\) が存在し、\(\log b-\log a=(b-a)/c\) である。".into();
        let mut alternative = variant(
            "alternative",
            vec![
                SolutionFlowBlock {
                    id: "integral-choice".into(),
                    block_type: "text".into(),
                    content: "上の定石の②（定積分表示）を用いる。".into(),
                    ..Default::default()
                },
                SolutionFlowBlock {
                    id: "integral-formula".into(),
                    block_type: "formula".into(),
                    latex: r"\log b-\log a=\int_a^b\dfrac{1}{x}\,dx".into(),
                    ..Default::default()
                },
            ],
        );
        alternative.solution = r"\(a<x<b\) では \(1/b<1/x<1/a\) だから積分する。".into();

        let latex = render_solution_flow_latex(common, vec![main, alternative]).unwrap();
        assert_eq!(latex.matches("\\begin{tcolorbox}").count(), 1);
        assert!(latex.contains("平均値の定理の利用"));
        assert!(latex.contains("定積分表示の利用"));
        assert!(latex.contains("【解法1】"));
        assert!(latex.contains("【別解1】"));
        assert!(!latex.contains("【解法1："));
        assert!(!latex.contains("【別解1："));
        assert!(latex.contains(r"\int_a^b"));
    }

    #[test]
    fn flow_without_patterns_is_valid() {
        let flow = vec![
            SolutionFlowBlock {
                id: "idea".into(),
                block_type: "text".into(),
                content: "対称性から置換後の形を比較する。".into(),
                ..Default::default()
            },
            SolutionFlowBlock {
                id: "calculation".into(),
                block_type: "formula".into(),
                latex: "x+y=1".into(),
                ..Default::default()
            },
        ];
        let latex = render_solution_flow_latex(vec![], vec![variant("main", flow)]).unwrap();
        assert!(!latex.contains("tcolorbox"));
        assert!(latex.contains("対称性"));
        assert!(latex.contains("x+y=1"));
    }

    #[test]
    fn formula_blocks_put_inner_math_environments_in_math_mode() {
        let aligned = display_formula(r"\begin{aligned}x&=1\\y&=2\end{aligned}");
        assert!(aligned.starts_with("\\[\n\\begin{aligned}"));
        assert!(aligned.ends_with("\\end{aligned}\n\\]\n"));

        let cases = display_formula(r"\begin{cases}x=1\\y=2\end{cases}");
        assert!(cases.starts_with("\\[\n\\begin{cases}"));

        let align = display_formula(r"\begin{align*}x&=1\end{align*}");
        assert!(align.starts_with("\\begin{align*}"));
        assert!(!align.starts_with("\\["));
    }

    #[test]
    fn formula_blocks_do_not_nest_existing_inline_math_delimiters() {
        assert_eq!(display_formula(r"\(x=1\)"), "\\[\nx=1\n\\]\n");
        assert_eq!(display_formula("$x=1$"), "\\[\nx=1\n\\]\n");
    }

    #[test]
    fn flow_spacing_is_compact_and_does_not_repeat_smallskip_between_blocks() {
        let flow = render_flow_blocks(&[
            SolutionFlowBlock {
                id: "text".into(),
                block_type: "text".into(),
                content: "着眼点を述べる。".into(),
                ..Default::default()
            },
            SolutionFlowBlock {
                id: "heading".into(),
                block_type: "heading".into(),
                text: "(1)".into(),
                ..Default::default()
            },
        ]);
        assert!(flow.contains("着眼点を述べる。\n\\par\n\\par\\smallskip"));
        assert!(!flow.contains("\\par\\smallskip\n\\par\\smallskip"));
        assert!(flow.contains("\\setlength{\\abovedisplayskip}{0.4em}"));
        assert!(flow.contains("\\setlength{\\belowdisplayskip}{0.4em}"));
        assert!(flow.contains("\\setlength{\\abovedisplayshortskip}{0.25em}"));
        assert!(flow.contains("\\setlength{\\belowdisplayshortskip}{0.25em}"));
    }

    #[test]
    fn text_connects_to_the_next_formula_without_an_extra_paragraph() {
        let flow = render_flow_blocks(&[
            SolutionFlowBlock {
                id: "reason".into(),
                block_type: "text".into(),
                content: "各成分の符号を調べる。".into(),
                ..Default::default()
            },
            SolutionFlowBlock {
                id: "signs".into(),
                block_type: "formula".into(),
                latex: "x_k\\geqq0".into(),
                ..Default::default()
            },
        ]);
        assert!(flow.contains("各成分の符号を調べる。\n\\[\nx_k\\geqq0"));
        assert!(!flow.contains("各成分の符号を調べる。\n\\par\n\\["));
    }

    #[test]
    fn qquad_condition_lists_are_stacked_for_narrow_columns() {
        let rendered = display_formula(
            r"z_k=r_k(\cos\theta_k+i\sin\theta_k),\qquad r_k=|z_k|>0,\qquad 0\leqq\theta_k\leqq\dfrac{\pi}{2}",
        );
        assert!(rendered.contains("\\begin{gathered}"));
        assert!(rendered.contains("\\\\\nr_k=|z_k|>0"));
        assert!(!rendered.contains(",\\qquad"));
    }

    #[test]
    fn short_two_term_formula_stays_on_one_line() {
        let rendered = display_formula(r"x_k\geqq0,\qquad y_k\geqq0");
        assert!(!rendered.contains("\\begin{gathered}"));
        assert!(rendered.contains(r"x_k\geqq0,\qquad y_k\geqq0"));
    }

    #[test]
    fn saved_flow_rejects_latex_incompatible_control_characters() {
        let flow = vec![SolutionFlowBlock {
            id: "invalid-control".into(),
            block_type: "formula".into(),
            latex: "x=1\0qquad y=2".into(),
            ..Default::default()
        }];
        let error = normalize_saved_flow_blocks(flow, &HashSet::new())
            .err()
            .expect("NULを含むFlowを保存してはいけない");
        assert!(error.contains("制御文字"), "{error}");
    }

    #[test]
    fn legacy_answer_only_has_no_forced_flow() {
        let mut legacy = variant("main", vec![]);
        legacy.solution = "既存答案".into();
        assert_eq!(render_teaching_flow_latex(&[], &[legacy]), "");
    }

    #[test]
    fn legacy_explanation_is_kept_when_structured_flow_is_absent() {
        let mut legacy = variant("main", vec![]);
        legacy.solution = "既存答案".into();
        legacy.explanation = Some("既存解説".into());
        assert_eq!(render_teaching_flow_latex(&[], &[legacy]), "既存解説");
    }

    #[test]
    fn existing_variant_can_gain_flow_without_losing_its_exam_answer() {
        let mut saved = variant(
            "main",
            vec![SolutionFlowBlock {
                id: "viewpoint".into(),
                block_type: "text".into(),
                content: "なぜこの方針を選ぶか。".into(),
                ..Default::default()
            }],
        );
        saved.solution = "保存済み答案".into();
        saved.explanation = Some("旧解説".into());
        let latex = render_teaching_flow_latex(&[], &[saved.clone()]);
        assert!(latex.contains("なぜこの方針"));
        assert!(!latex.contains("旧解説"));
        assert_eq!(saved.solution, "保存済み答案");
    }

    #[test]
    fn main_and_alternative_can_use_different_patterns() {
        let first = pattern_block("first", vec![101], 1);
        let mut second = pattern_block("second", vec![102], 1);
        second.pattern_id = Some(43);
        second.snapshot.as_mut().unwrap().uuid = "second-pattern".into();
        second.snapshot.as_mut().unwrap().title = "別の定石".into();
        let latex = render_solution_flow_latex(
            vec![],
            vec![
                variant("main", vec![first]),
                variant("alternative", vec![second]),
            ],
        )
        .unwrap();
        assert_eq!(latex.matches("\\begin{tcolorbox}").count(), 2);
        assert!(latex.contains("別の定石"));
    }

    #[test]
    fn adjacent_duplicate_patterns_are_removed() {
        let flow = normalize_saved_flow_blocks(
            vec![
                pattern_block("first", vec![101], 1),
                pattern_block("again", vec![102], 1),
            ],
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(flow.len(), 1);
    }

    #[test]
    fn the_same_pattern_can_be_reused_after_new_mathematical_context() {
        let flow = normalize_saved_flow_blocks(
            vec![
                pattern_block("first", vec![101], 1),
                SolutionFlowBlock {
                    id: "first-application".into(),
                    block_type: "text".into(),
                    content: "ここでは、平均値の定理を用いる。".into(),
                    ..Default::default()
                },
                SolutionFlowBlock {
                    id: "next-part".into(),
                    block_type: "heading".into(),
                    text: "(2)".into(),
                    ..Default::default()
                },
                pattern_block("again", vec![102], 1),
            ],
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(flow.len(), 4);
        assert_eq!(
            flow.iter()
                .filter(|block| block.block_type == "pattern")
                .count(),
            2
        );
    }

    #[test]
    fn a_common_pattern_can_be_reused_later_in_a_variant() {
        let common = vec![pattern_block("common", vec![101], 1)];
        let variant = variant(
            "main",
            vec![
                SolutionFlowBlock {
                    id: "next-part".into(),
                    block_type: "heading".into(),
                    text: "(2)".into(),
                    ..Default::default()
                },
                pattern_block("again", vec![102], 1),
            ],
        );
        let latex = render_solution_flow_latex(common, vec![variant]).unwrap();
        assert_eq!(latex.matches("\\begin{tcolorbox}").count(), 2);
    }

    #[test]
    fn snapshot_version_is_preserved() {
        let block = pattern_block("old", vec![101], 2);
        let flow = normalize_saved_flow_blocks(vec![block], &HashSet::new()).unwrap();
        assert_eq!(flow[0].pattern_version, Some(2));
        assert_eq!(flow[0].snapshot.as_ref().unwrap().version, 2);
    }

    #[test]
    fn unknown_candidate_is_rejected() {
        let result =
            normalize_saved_flow_blocks(vec![pattern_block("bad", vec![999], 1)], &HashSet::new());
        let error = match result {
            Ok(_) => panic!("存在しないCandidateが受理されました"),
            Err(error) => error,
        };
        assert!(error.contains("存在しないCandidate"));
    }

    #[test]
    fn ai_unknown_pattern_id_is_rejected_before_saving() {
        let state = AppState::new(
            rusqlite::Connection::open_in_memory().unwrap(),
            std::env::temp_dir().join("kyozai-solution-flow-test"),
        );
        let result = hydrate_ai_flow_blocks(
            &state,
            vec![SolutionFlowBlock {
                id: "missing-pattern".into(),
                block_type: "pattern".into(),
                pattern_id: Some(999_999),
                used_strategy_ids: vec![],
                ..Default::default()
            }],
            &HashSet::new(),
        );
        let error = match result {
            Ok(_) => panic!("存在しないPatternが受理されました"),
            Err(error) => error,
        };
        assert!(error.contains("存在しないpatternId"));
    }
}
