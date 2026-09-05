//! 定石カードのLaTeX描画。
//!
//! 定石の正本データ（PatternSnapshot）は教材用のLaTeX文字列を持たない。
//! 教材・解説・部品へ出すときだけ、ここで tcolorbox のカードへ変換する。
//!
//! ```text
//! Pattern Data → (このモジュール) → tcolorbox LaTeX → PDF
//! ```
//!
//! カードの見た目を変えるときは、このファイルだけを直せばよい。
//! upLaTeX + dvipdfmx で組めることを前提とし、tcolorbox のプリアンブル追加は
//! latex.rs の ensure_tcolorbox_support が本文を見て自動で行う。

use crate::models::{PatternSnapshot, PatternStrategyInput};

/// itemize の間隔指定。項目が詰まりすぎないようにする。
const ITEMIZE_LENGTHS: &str = concat!(
    "  \\setlength{\\itemsep}{5pt}\n",
    "  \\setlength{\\parskip}{0pt}\n",
    "  \\setlength{\\itemindent}{0pt}\n",
    "  \\setlength{\\labelsep}{5pt}\n"
);

/// 本文中の数式はそのまま通すため、エスケープはしない。
/// 空行はカードの段落を崩すので詰める。
fn text_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// tcolorboxのtitle={...}へ入れても壊れないように、中括弧の対応だけ整える。
/// 数式やマクロは教材本文と同じくそのまま通す。
fn title_text(source: &str) -> String {
    let mut depth: i64 = 0;
    let mut out = String::new();
    for character in source.trim().chars() {
        match character {
            '{' => {
                depth += 1;
                out.push(character);
            }
            '}' if depth > 0 => {
                depth -= 1;
                out.push(character);
            }
            // 対応の取れない閉じ括弧は落とす（オプション全体の解析が壊れるため）。
            '}' => {}
            '\n' | '\r' => out.push(' '),
            _ => out.push(character),
        }
    }
    for _ in 0..depth {
        out.push('}');
    }
    out.trim().to_string()
}

fn starts_display(line: &str) -> bool {
    line.trim_start().starts_with("\\[")
}

fn ends_display(line: &str) -> bool {
    line.trim_end().ends_with("\\]")
}

/// 1つの \item の中身を組み立てる。
/// 別行立ての数式の前後に \\ を置くと「行がないのに改行した」というエラーになるため、
/// その境目だけ改行命令を省く。
fn join_item_lines(lines: &[String], indent: &str) -> String {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            if !starts_display(line) && !ends_display(&lines[index - 1]) {
                out.push_str("\\\\");
            }
            out.push('\n');
            out.push_str(indent);
        }
        out.push_str(line);
    }
    out
}

/// 候補1件分の行。空の項目は行ごと出さない。
fn entry_lines(entry: &PatternStrategyInput) -> Option<Vec<String>> {
    let title = entry.title.trim();
    if title.is_empty() {
        return None;
    }
    let mut lines = vec![title.to_string()];
    lines.extend(text_lines(&entry.description));
    Some(lines)
}

/// 定石スナップショットを、解説・部品・教材へそのまま貼れる tcolorbox のカードにする。
///
/// カードに出すのはタイトルと候補となる考え方だけ。
/// 候補が1つならそのまま、複数なら itemize の項目として並べる。
pub fn render_pattern_card(snapshot: &PatternSnapshot) -> String {
    render_pattern_card_with_usage(snapshot, &[])
}

/// Pattern全体を必ず表示する。`used_strategy_ids` は解法との対応を追跡する
/// 保存情報であり、カード本文へ「今回使用」等の印は付けない。
pub fn render_pattern_card_with_usage(
    snapshot: &PatternSnapshot,
    _used_strategy_ids: &[i64],
) -> String {
    let title = title_text(&snapshot.title);
    let title_option = if title.is_empty() {
        String::new()
    } else {
        format!("[title={{{title}}}]")
    };
    let mut out = format!("\\begin{{tcolorbox}}{title_option}\n");

    let entries: Vec<Vec<String>> = snapshot.strategies.iter().filter_map(entry_lines).collect();
    match entries.len() {
        0 => {}
        1 => {
            // 候補が1つなら箇条書きにせず、そのまま本文として置く。
            out.push_str(&format!("  {}\n", join_item_lines(&entries[0], "  ")));
        }
        _ => {
            out.push_str("  \\begin{itemize}\n");
            out.push_str(ITEMIZE_LENGTHS);
            for entry in &entries {
                out.push_str(&format!("  \\item {}\n", join_item_lines(entry, "  ")));
            }
            out.push_str("  \\end{itemize}\n");
        }
    }

    out.push_str("\\end{tcolorbox}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PatternFacets;

    fn strategy(title: &str, description: &str) -> PatternStrategyInput {
        PatternStrategyInput {
            id: None,
            parent_strategy_id: None,
            title: title.into(),
            description: description.into(),
            condition: String::new(),
            reasoning: String::new(),
            branch_label: String::new(),
            sort_order: 1,
        }
    }

    fn snapshot() -> PatternSnapshot {
        PatternSnapshot {
            version: 1,
            uuid: "uuid".into(),
            title: "関数値の差 \\(f(b)-f(a)\\) の扱い".into(),
            // 概要・状況・基本原則・注意・例はカードに出さない。
            summary: "カードに出さない概要".into(),
            pattern_type: "strategy".into(),
            situation: "カードに出さない状況".into(),
            principle: "カードに出さない基本原則".into(),
            cautions: "カードに出さない注意".into(),
            examples: "カードに出さない例".into(),
            source_note: "出典".into(),
            tags: vec![],
            facets: PatternFacets::default(),
            strategies: vec![
                strategy(
                    "平均値の定理の利用",
                    "関数 \\(y=f(x)\\) が微分可能で \\(a<b\\) であるとき\n\\[\\begin{cases}\\dfrac{f(b)-f(a)}{b-a}=f'(c)\\\\a<c<b\\end{cases}\\]\nとなる \\(c\\) が存在する",
                ),
                strategy(
                    "定積分の評価の利用",
                    "\\(f(b)-f(a)=\\left[f(x)\\right]_a^b\\) を作り、\\(a\\leqq x\\leqq b\\) において \\(f'(x)\\) を単調性や最大最小等から評価してはさむ",
                ),
            ],
        }
    }

    #[test]
    fn several_entries_are_listed_as_items() {
        let tex = render_pattern_card(&snapshot());
        assert!(tex.starts_with("\\begin{tcolorbox}[title={関数値の差 \\(f(b)-f(a)\\) の扱い}]"));
        assert_eq!(tex.matches("\\begin{itemize}").count(), 1);
        assert_eq!(tex.matches("\\end{itemize}").count(), 1);
        assert_eq!(tex.matches("\\item ").count(), 2);
        assert!(tex.contains("\\setlength{\\itemsep}{5pt}"));
        assert!(tex.contains("\\item 平均値の定理の利用"));
        // 数式はエスケープせずそのまま通す。
        assert!(tex.contains("\\dfrac{f(b)-f(a)}{b-a}=f'(c)"));
        assert!(tex.trim_end().ends_with("\\end{tcolorbox}"));
    }

    #[test]
    fn a_single_entry_is_inserted_without_an_itemize() {
        let mut snapshot = snapshot();
        snapshot.strategies.truncate(1);
        let tex = render_pattern_card(&snapshot);
        assert!(
            !tex.contains("\\begin{itemize}"),
            "候補が1つなら箇条書きにしない"
        );
        assert!(!tex.contains("\\item"));
        assert!(tex.contains("平均値の定理の利用"));
        assert!(tex.contains("\\dfrac{f(b)-f(a)}{b-a}=f'(c)"));
    }

    #[test]
    fn usage_metadata_does_not_change_the_pattern_card() {
        let mut snapshot = snapshot();
        snapshot.strategies[0].id = Some(101);
        snapshot.strategies[1].id = Some(102);
        let with_usage = render_pattern_card_with_usage(&snapshot, &[101]);
        let without_usage = render_pattern_card(&snapshot);
        assert_eq!(with_usage, without_usage);
        assert!(!with_usage.contains("今回使用"));
        assert!(with_usage.contains("平均値の定理の利用"));
        assert!(with_usage.contains("定積分の評価の利用"));
        assert_eq!(with_usage.matches("\\item ").count(), 2);
    }

    #[test]
    fn only_the_candidate_strategies_reach_the_card() {
        let tex = render_pattern_card(&snapshot());
        for hidden in [
            "カードに出さない概要",
            "カードに出さない状況",
            "カードに出さない基本原則",
            "カードに出さない注意",
            "カードに出さない例",
        ] {
            assert!(!tex.contains(hidden), "{hidden} はカードへ出さない");
        }
        assert!(!tex.contains("こう見えたら"));
        assert!(!tex.contains("基本の考え方"));
        assert!(!tex.contains("\\textbf{注意}"));
        assert!(!tex.contains("\\textbf{例}"));
    }

    #[test]
    fn display_math_is_not_preceded_or_followed_by_a_line_break_command() {
        let tex = render_pattern_card(&snapshot());
        // 「\\ の直後に別行立て数式」「別行立て数式の直後に \\」はLaTeXがエラーにする。
        assert!(
            !tex.contains("\\\\\n  \\["),
            "別行立て数式の前に改行命令を置かない"
        );
        assert!(
            !tex.contains("\\]\\\\"),
            "別行立て数式の直後に改行命令を置かない"
        );
        assert!(
            tex.contains("平均値の定理の利用\\\\"),
            "見出しと本文は改行でつなぐ"
        );
    }

    #[test]
    fn broken_title_braces_are_repaired() {
        let mut snapshot = snapshot();
        snapshot.title = "壊れた}見出し\\(\\frac{1}{2}".into();
        let tex = render_pattern_card(&snapshot);
        // 対応の取れない閉じ括弧を落とし、開いたままの括弧を閉じる。
        assert!(tex.contains("title={壊れた見出し\\(\\frac{1}{2}}"));
    }

    #[test]
    fn a_card_without_entries_does_not_emit_an_empty_itemize() {
        let mut snapshot = snapshot();
        snapshot.strategies = vec![];
        let tex = render_pattern_card(&snapshot);
        // \begin{itemize} が \item なしで出るとLaTeXがエラーになる。
        assert!(!tex.contains("\\begin{itemize}"));
        assert!(tex.contains("\\begin{tcolorbox}"));
        assert!(tex.trim_end().ends_with("\\end{tcolorbox}"));
    }

    #[test]
    fn entry_extras_are_not_rendered_as_separate_lines() {
        // 適用条件・理由は項目として持たせない。必要な情報は本文へ入れる。
        let mut snapshot = snapshot();
        snapshot.strategies[0].condition = "旧データの適用条件".into();
        snapshot.strategies[0].reasoning = "旧データの理由".into();
        let tex = render_pattern_card(&snapshot);
        assert!(!tex.contains("旧データの適用条件"));
        assert!(!tex.contains("旧データの理由"));
        assert!(!tex.contains("使う場面"));
        assert!(!tex.contains("なぜ有効か"));
    }
}
