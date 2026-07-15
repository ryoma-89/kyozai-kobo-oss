use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::params;
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use super::templates::{DEFAULT_ANSWER_TEMPLATE, DEFAULT_PROBLEM_TEMPLATE, MARKER_END, MARKER_START};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const MAX_TEX_BYTES: usize = 5 * 1024 * 1024;
const MAX_COMPILE_LOG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMPILE_DIR_BYTES: u64 = 160 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;

fn compile_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// LaTeXの特殊文字をエスケープ（タイトル等の非LaTeXフィールド用）
pub fn escape_latex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '#' => out.push_str("\\#"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '_' => out.push_str("\\_"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            _ => out.push(c),
        }
    }
    out
}

fn sanitize_filename(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() {
        out = "教材".to_string();
    }
    out
}

// ---- 本文生成 ----

/// 縦線（罫線）付き2段組の開始・終了
const TWO_COL_BEGIN: &str = "{\\setlength{\\columnseprule}{0.4pt}%\n\\begin{multicols}{2}\n";
const TWO_COL_END: &str = "\\end{multicols}}\n";

fn difficulty_badge_lines(item: &ProjectItem, settings: &ProjectSettings) -> Vec<String> {
    let show_rank = settings.difficulty_display != "none";
    let show_required = settings.required_display != "none" && item.snap_is_required;
    let mut lines = vec![];
    if show_required {
        lines.push("★".to_string());
    }
    if show_rank {
        if let Some(rank) = &item.snap_difficulty_rank {
            if !rank.trim().is_empty() {
                lines.push(escape_latex(rank));
            }
        }
    }
    lines
}

fn difficulty_right_badge_tex(item: &ProjectItem, settings: &ProjectSettings) -> String {
    let parts = difficulty_badge_lines(item, settings);
    if parts.is_empty() {
        String::new()
    } else {
        format!("\\hfill {{\\small {}}}", parts.join("\\;"))
    }
}

fn difficulty_number_side_badge_tex(item: &ProjectItem, settings: &ProjectSettings) -> String {
    let parts = difficulty_badge_lines(item, settings);
    if parts.is_empty() {
        String::new()
    } else {
        format!("{{\\scriptsize {}}}", parts.join(""))
    }
}

fn problem_head_title_tex(head: &str) -> String {
    let mut title = head.trim();
    if let Some(rest) = title.strip_prefix("\\par\\medskip") {
        title = rest.trim_start();
    }
    if let Some(rest) = title.strip_prefix("\\noindent") {
        title = rest.trim_start();
    }
    if let Some(rest) = title.strip_suffix("\\par") {
        title = rest.trim_end();
    }
    title.to_string()
}

fn answer_statement_box_tex(head: &str, statement: &str) -> String {
    let title = problem_head_title_tex(head);
    let title_option = if title.is_empty() {
        String::new()
    } else {
        format!(",title={{{}}}", title)
    };
    let mut out = format!(
        "\\par\\medskip\n\\begin{{tcolorbox}}[enhanced,width=\\linewidth,colback=white,colframe=black,coltitle=black,colbacktitle=white,fonttitle=\\bfseries,boxrule=0.65pt,arc=0pt,outer arc=0pt,boxsep=0pt,left=6pt,right=6pt,top=7pt,bottom=5pt,before skip=0pt,after skip=0.5em,attach boxed title to top left={{xshift=4mm,yshift*=-\\tcboxedtitleheight/2}},boxed title style={{colback=white,colframe=white,boxrule=0pt,arc=0pt,outer arc=0pt,boxsep=0pt,left=2pt,right=2pt,top=0pt,bottom=0pt}}{}]\n",
        title_option
    );
    out.push_str(statement);
    out.push_str("\n\\end{tcolorbox}\n");
    out
}

fn ensure_tcolorbox_support(mut doc: String, needs_tcolorbox: bool, needs_skins: bool) -> String {
    if !needs_tcolorbox {
        return doc;
    }
    let has_tcolorbox = doc.contains("\\usepackage{tcolorbox}");
    let has_skins = doc.contains("\\tcbuselibrary") && doc.contains("skins");
    let mut insertions = String::new();
    if !has_tcolorbox {
        insertions.push_str("\\usepackage{tcolorbox}\n");
    }
    if needs_skins && !has_skins {
        insertions.push_str("\\tcbuselibrary{skins}\n");
    }
    if insertions.is_empty() {
        return doc;
    }
    if let Some(pos) = doc.find("\\begin{document}") {
        doc.insert_str(pos, &insertions);
    }
    doc
}

fn append_part_to_bodies(item: &ProjectItem, body: &mut String, answer_plain: &mut String, answer_inline: &mut String) {
    if item.snap_part_output_target == "none" {
        return;
    }
    let mut latex = if item.snap_part_type == "page_break" && item.content.trim().is_empty() {
        "\\newpage".to_string()
    } else {
        item.content.clone()
    };
    if !latex.ends_with('\n') {
        latex.push('\n');
    }
    latex.push('\n');

    if item.snap_part_output_target == "problems" || item.snap_part_output_target == "both" {
        body.push_str(&latex);
    }
    if item.snap_part_output_target == "answers" || item.snap_part_output_target == "both" {
        answer_plain.push_str(&latex);
        answer_inline.push_str(&latex);
    }
}

pub struct Bodies {
    /// 問題冊子用本文（問題文のみ）
    pub body: String,
    /// 解答冊子用本文（解説は含まない）
    pub answer_plain: String,
    /// 解答冊子用本文（解説をインライン挿入）
    pub answer_inline: String,
    /// {{EXPLANATION_BODY}} 用の解説一覧
    pub explanation: String,
}

/// 教材項目から各冊子の本文LaTeXを組み立てる
pub fn render_bodies(items: &[ProjectItem], settings: &ProjectSettings) -> Bodies {
    let mut body = String::new();
    let mut answer_plain = String::new();
    let mut answer_inline = String::new();
    let mut explanation = String::new();
    let mut n: i64 = 0;
    // 章の状態: 現在の章番号（番号付き章のみカウント）と、現在の章が番号付きか
    let mut chapter_no: i64 = 0;
    let mut chapter_numbered = false;

    for item in items {
        match item.item_type.as_str() {
            "heading" => {
                // 章(section) / 節(subsection)。番号は全体設定＋見出しごとの設定の両方がONのとき
                let cmd = if item.heading_level >= 2 { "subsection" } else { "section" };
                let esc = escape_latex(&item.content);
                let numbered = settings.number_headings && item.heading_numbered;

                if item.heading_level <= 1 {
                    if settings.reset_numbering_per_chapter {
                        n = 0; // 章ごとに問題番号をリセット
                    }
                    if numbered {
                        chapter_no += 1;
                        chapter_numbered = true;
                    } else {
                        chapter_numbered = false;
                    }
                }

                // 問題側の見出し
                let s = if numbered {
                    format!("\\{}{{{}}}\n\n", cmd, esc)
                } else if settings.show_toc {
                    // 番号なしでも目次に載せる
                    format!("\\{}*{{{}}}\n\\addcontentsline{{toc}}{{{}}}{{{}}}\n\n", cmd, esc, cmd, esc)
                } else {
                    format!("\\{}*{{{}}}\n\n", cmd, esc)
                };
                body.push_str(&s);

                // 解答側の見出し: 常に番号なしのコマンドを使い、章番号がある場合は
                // 見出し・目次とも「1　第1章」の形式にする（問題側との区別は「解答編」区切りが担う）
                let ans_heading_text = if numbered && item.heading_level <= 1 {
                    format!("{}　{}", chapter_no, esc)
                } else {
                    esc.clone()
                };
                let sa = if settings.show_toc {
                    format!(
                        "\\{}*{{{}}}\n\\addcontentsline{{toc}}{{{}}}{{{}}}\n\n",
                        cmd, ans_heading_text, cmd, ans_heading_text
                    )
                } else {
                    format!("\\{}*{{{}}}\n\n", cmd, ans_heading_text)
                };
                answer_plain.push_str(&sa);
                answer_inline.push_str(&sa);
            }
            "text" => {
                body.push_str(&item.content);
                body.push_str("\n\n");
                answer_plain.push_str(&item.content);
                answer_plain.push_str("\n\n");
                answer_inline.push_str(&item.content);
                answer_inline.push_str("\n\n");
            }
            "pagebreak" => {
                body.push_str("\\newpage\n\n");
                answer_plain.push_str("\\newpage\n\n");
                answer_inline.push_str("\\newpage\n\n");
            }
            "part" => {
                append_part_to_bodies(item, &mut body, &mut answer_plain, &mut answer_inline);
            }
            "problem" => {
                n += 1;
                let head = if settings.auto_number {
                    // 番号付き章の中では「章番号-連番」形式（例: 問題2-1）
                    let n_str = if settings.reset_numbering_per_chapter && chapter_numbered {
                        format!("{}-{}", chapter_no, n)
                    } else {
                        n.to_string()
                    };
                    let label = settings.number_format.replace("{n}", &n_str);
                    let number_badge = difficulty_number_side_badge_tex(item, settings);
                    let number_badge = if settings.difficulty_display == "top_right" || number_badge.is_empty() {
                        String::new()
                    } else {
                        format!("\\nobreak\\hspace{{0.15em}}{}", number_badge)
                    };
                    let right_badge = if settings.difficulty_display == "top_right" {
                        difficulty_right_badge_tex(item, settings)
                    } else {
                        String::new()
                    };
                    format!(
                        "\\par\\medskip\n\\noindent\\textbf{{{}}}{}{}\\par\n",
                        escape_latex(&label),
                        number_badge,
                        right_badge
                    )
                } else {
                    let badge = if settings.difficulty_display == "top_right" {
                        difficulty_right_badge_tex(item, settings)
                    } else {
                        difficulty_number_side_badge_tex(item, settings)
                    };
                    if badge.is_empty() {
                        "\\par\\medskip\n\\noindent\n".to_string()
                    } else {
                        format!("\\par\\medskip\n\\noindent{}\\par\n", badge)
                    }
                };

                body.push_str(&head);
                body.push_str(&item.snap_statement);
                body.push_str("\n\\par\\medskip\n\n");

                let has_expl = settings.include_explanation && !item.snap_explanation.trim().is_empty();
                // 「解答部分のみ2段組」: 問題文は1段のまま、【解答】ブロックだけを縦線付きmulticolsで囲む
                let answer_only_cols = settings.two_column_mode == "answer_only";

                let ans_head = if settings.include_statement_in_answers && settings.box_statement_in_answers {
                    answer_statement_box_tex(&head, &item.snap_statement)
                } else if settings.include_statement_in_answers {
                    let mut plain = head.clone();
                    plain.push_str(&item.snap_statement);
                    plain.push_str("\n\\par\\vspace{0.5em}\n");
                    plain
                } else {
                    head.clone()
                };

                let mut ans_core_plain = String::from("\\noindent\\textbf{【解答】}\\par\n");
                ans_core_plain.push_str(&item.snap_answer);
                ans_core_plain.push('\n');
                let mut ans_core_inline = ans_core_plain.clone();
                if has_expl {
                    ans_core_inline.push_str("\\par\\vspace{0.5em}\n\\noindent\\textbf{【解説】}\\par\n");
                    ans_core_inline.push_str(&item.snap_explanation);
                    ans_core_inline.push('\n');
                    explanation.push_str(&head);
                    explanation.push_str("\\noindent\\textbf{【解説】}\\par\n");
                    explanation.push_str(&item.snap_explanation);
                    explanation.push_str("\n\\par\\medskip\n\n");
                }

                let wrap = |core: &str| {
                    if answer_only_cols {
                        format!("{}{}{}", TWO_COL_BEGIN, core, TWO_COL_END)
                    } else {
                        core.to_string()
                    }
                };
                answer_plain.push_str(&ans_head);
                answer_plain.push_str(&wrap(&ans_core_plain));
                answer_plain.push_str("\\par\\medskip\n\n");
                answer_inline.push_str(&ans_head);
                answer_inline.push_str(&wrap(&ans_core_inline));
                answer_inline.push_str("\\par\\medskip\n\n");

                if settings.page_break_per_problem {
                    body.push_str("\\newpage\n\n");
                    answer_plain.push_str("\\newpage\n\n");
                    answer_inline.push_str("\\newpage\n\n");
                }
            }
            _ => {}
        }
    }
    Bodies {
        body,
        answer_plain,
        answer_inline,
        explanation,
    }
}

/// % APP_BODY_START / % APP_BODY_END の間に本文を挿入する
fn insert_at_markers(tpl: &str, body: &str) -> Option<String> {
    let start = tpl.find(MARKER_START)?;
    let end = tpl.find(MARKER_END)?;
    if end < start {
        return None;
    }
    let after_start = start + MARKER_START.len();
    Some(format!("{}\n{}\n{}", &tpl[..after_start], body, &tpl[end..]))
}

/// テンプレートに本文とプレースホルダを適用して完成した .tex を返す
/// kind: "problems" | "answers" | "combined"（問題＋解答の合本）
pub fn render_document(
    tpl: &str,
    kind: &str,
    project_name: &str,
    settings: &ProjectSettings,
    bodies: &Bodies,
) -> String {
    let is_answers = kind == "answers";
    let is_combined = kind == "combined";

    // 解答本文: テンプレートが {{EXPLANATION_BODY}} を持つなら解説は分離、なければインライン
    // （合本ではプレースホルダに関わらず常にインライン）
    let mut answer_body = if !is_combined && tpl.contains("{{EXPLANATION_BODY}}") {
        bodies.answer_plain.clone()
    } else {
        bodies.answer_inline.clone()
    };
    // 「問題＋解答全体を2段組」: 解答本文全体を縦線付きmulticolsで囲む
    if (is_answers || is_combined) && settings.two_column_mode == "all" {
        answer_body = format!("{}{}{}", TWO_COL_BEGIN, answer_body, TWO_COL_END);
    }

    let mut main_body = if is_answers {
        answer_body.clone()
    } else if is_combined {
        // 合本: 問題冊子の本文 → 改ページ → 「解答」見出し → 解答本文
        let toc_line = if settings.show_toc {
            "\\addcontentsline{toc}{section}{──── 解答編 ────}\n"
        } else {
            ""
        };
        format!(
            "{}\n\\clearpage\n{}\\begin{{center}}{{\\LARGE \\textbf{{解答}}}}\\end{{center}}\n\\par\\medskip\n\n{}",
            bodies.body, toc_line, answer_body
        )
    } else {
        bodies.body.clone()
    };

    // 目次: テンプレートに {{TOC}} があればその位置、無ければ本文の先頭に挿入
    let has_toc_ph = tpl.contains("{{TOC}}");
    if settings.show_toc && !has_toc_ph {
        main_body = format!("\\tableofcontents\n\\par\\bigskip\n\n{}", main_body);
    }
    let needs_tcolorbox = main_body.contains("\\begin{tcolorbox}");
    let needs_tcolorbox_skins = main_body.contains("enhanced");

    let mut doc = insert_at_markers(tpl, &main_body).unwrap_or_else(|| tpl.to_string());
    doc = doc.replace("{{TOC}}", if settings.show_toc { "\\tableofcontents" } else { "" });

    // 本文プレースホルダ
    // {{BODY}}: 問題冊子/合本では main_body（目次込み）。解答冊子では {{ANSWER_BODY}} が
    // 無いテンプレートに限り解答本文を入れる（両方持つテンプレートでは問題本文のまま）
    let body_for_ph = if is_answers {
        if doc.contains("{{ANSWER_BODY}}") { bodies.body.clone() } else { main_body.clone() }
    } else {
        main_body.clone()
    };
    doc = doc.replace("{{BODY}}", &body_for_ph);
    doc = doc.replace("{{ANSWER_BODY}}", if is_answers { &main_body } else { "" });
    doc = doc.replace(
        "{{EXPLANATION_BODY}}",
        if is_answers { bodies.explanation.as_str() } else { "" },
    );

    // メタ情報プレースホルダ
    let title = if settings.booklet_title.trim().is_empty() {
        project_name.to_string()
    } else {
        settings.booklet_title.clone()
    };
    let header_left = if settings.header_left.trim().is_empty() {
        title.clone()
    } else {
        settings.header_left.clone()
    };
    let header_right = if settings.header_right.trim().is_empty() {
        settings.date_str.clone()
    } else {
        settings.header_right.clone()
    };

    let mut name_parts: Vec<String> = vec![];
    if !settings.target.trim().is_empty() {
        name_parts.push(escape_latex(&settings.target));
    }
    if !settings.date_str.trim().is_empty() {
        name_parts.push(escape_latex(&settings.date_str));
    }
    if settings.show_name_field && !is_answers {
        name_parts.push("氏名\\ \\underline{\\hspace{5cm}}".to_string());
    }
    let name_field = if name_parts.is_empty() {
        String::new()
    } else {
        format!("\\begin{{flushright}}\n{}\n\\end{{flushright}}", name_parts.join(" \\quad "))
    };

    if is_answers && !doc.contains("{{ANSWER_TITLE}}") {
        doc = doc.replace("{{TITLE}}　解答", "{{ANSWER_TITLE}}");
        doc = doc.replace("{{TITLE}} 解答", "{{ANSWER_TITLE}}");
    }

    // タイトル・ヘッダーの表示設定
    let answer_title = if settings.show_title {
        format!("{}　解答", escape_latex(&title))
    } else {
        String::new()
    };
    doc = doc.replace("{{ANSWER_TITLE}}", &answer_title);
    if settings.show_title {
        doc = doc.replace("{{TITLE}}", &escape_latex(&title));
        doc = doc.replace("{{SUBTITLE}}", &escape_latex(&settings.subtitle));
    } else {
        doc = doc.replace("{{TITLE}}", "");
        doc = doc.replace("{{SUBTITLE}}", "");
    }
    if settings.show_header {
        doc = doc.replace("{{HEADER_LEFT}}", &escape_latex(&header_left));
        doc = doc.replace("{{HEADER_RIGHT}}", &escape_latex(&header_right));
    } else {
        doc = doc.replace("{{HEADER_LEFT}}", "");
        doc = doc.replace("{{HEADER_RIGHT}}", "");
        // fancyhdr使用時はヘッダーの罫線も消す
        if doc.contains("fancyhdr") {
            doc = doc.replace(
                "\\begin{document}",
                "\\fancyhead{}\\renewcommand{\\headrulewidth}{0pt}\n\\begin{document}",
            );
        }
    }
    doc = doc.replace("{{TARGET}}", &escape_latex(&settings.target));
    doc = doc.replace("{{DATE}}", &escape_latex(&settings.date_str));
    doc = doc.replace("{{NAME_FIELD}}", &name_field);
    doc = doc.replace("{{PAGE_BREAK}}", "\\newpage");
    ensure_tcolorbox_support(doc, needs_tcolorbox, needs_tcolorbox_skins)
}

// ---- プロジェクトデータの読み込み ----

pub struct TplSnapshot {
    pub name: String,
    pub problem: String,
    pub answer: String,
    pub assets: Vec<SnapAttachment>,
    pub compile_method: String,
}

struct ProjectData {
    name: String,
    settings: ProjectSettings,
    items: Vec<ProjectItem>,
    tpl: TplSnapshot,
}

fn load_project_data(conn: &rusqlite::Connection, project_id: i64) -> Result<ProjectData, String> {
    let (name, snap_name, snap_base, snap_problem, snap_answer, snap_assets, snap_compile): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT name, snap_tpl_name, snap_tpl_base, snap_tpl_problem, snap_tpl_answer, snap_tpl_assets, snap_tpl_compile
             FROM projects WHERE id=?1",
            params![project_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .map_err(err_str)?;
    let settings = super::projects::settings_of(conn, project_id).map_err(err_str)?;
    let items = super::projects::items_of(conn, project_id)?;

    // フォールバック: 個別テンプレート → 共通テンプレート → 組み込み既定
    let problem = if !snap_problem.trim().is_empty() {
        snap_problem
    } else if !snap_base.trim().is_empty() {
        snap_base.clone()
    } else {
        DEFAULT_PROBLEM_TEMPLATE.to_string()
    };
    let answer = if !snap_answer.trim().is_empty() {
        snap_answer
    } else if !snap_base.trim().is_empty() {
        snap_base
    } else {
        DEFAULT_ANSWER_TEMPLATE.to_string()
    };

    Ok(ProjectData {
        name,
        settings,
        items,
        tpl: TplSnapshot {
            name: snap_name,
            problem,
            answer,
            assets: serde_json::from_str(&snap_assets).unwrap_or_default(),
            compile_method: snap_compile,
        },
    })
}

fn booklet_suffix(kind: &str) -> &'static str {
    match kind {
        "answers" => "解答",
        "combined" => "合本",
        _ => "問題",
    }
}

fn build_project_tex(data: &ProjectData, kind: &str) -> String {
    let bodies = render_bodies(&data.items, &data.settings);
    let tpl = if kind == "answers" { &data.tpl.answer } else { &data.tpl.problem };
    render_document(tpl, kind, &data.name, &data.settings, &bodies)
}

// ---- TeXコマンド解決 ----

fn get_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", params![key], |r| r.get(0))
        .ok()
        .filter(|v: &String| !v.trim().is_empty())
}

/// uplatex / dvipdfmx の実行ファイルパスを解決する
pub fn resolve_tex_cmd(conn: &rusqlite::Connection, name: &str, key: &str) -> Option<PathBuf> {
    if let Some(p) = get_setting(conn, key) {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(dir) = get_setting(conn, "tex_bin_dir") {
        let pb = PathBuf::from(dir).join(format!("{}.exe", name));
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(out) = no_window(Command::new("where.exe").arg(name)).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = s.lines().next() {
                let pb = PathBuf::from(line.trim());
                if pb.exists() {
                    return Some(pb);
                }
            }
        }
    }
    for base in ["C:\\texlive", "D:\\texlive"] {
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut years: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            years.sort();
            years.reverse();
            for y in years {
                for sub in ["windows", "win32"] {
                    let pb = y.join("bin").join(sub).join(format!("{}.exe", name));
                    if pb.exists() {
                        return Some(pb);
                    }
                }
            }
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let pb = PathBuf::from(local)
            .join("Programs\\MiKTeX\\miktex\\bin\\x64")
            .join(format!("{}.exe", name));
        if pb.exists() {
            return Some(pb);
        }
    }
    None
}

pub fn detect_tex(state: &AppState) -> Result<TexDetection, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    Ok(TexDetection {
        uplatex_path: resolve_tex_cmd(&conn, "uplatex", "uplatex_path").map(|p| p.to_string_lossy().to_string()),
        dvipdfmx_path: resolve_tex_cmd(&conn, "dvipdfmx", "dvipdfmx_path").map(|p| p.to_string_lossy().to_string()),
    })
}

// ---- 出力・コンパイル ----

pub fn output_dir(conn: &rusqlite::Connection, state: &AppState) -> PathBuf {
    if let Some(dir) = get_setting(conn, "output_dir") {
        let pb = PathBuf::from(dir);
        if std::fs::create_dir_all(&pb).is_ok() {
            return pb;
        }
    }
    // 既定はドキュメントフォルダ内「教材工房」（AppData配下はPDFビューアが開けない環境があるため）
    if let Some(docs) = &state.documents_dir {
        let pb = docs.join("教材工房");
        if std::fs::create_dir_all(&pb).is_ok() {
            return pb;
        }
    }
    let pb = state.data_dir.join("output");
    std::fs::create_dir_all(&pb).ok();
    pb
}

/// スナップショットの添付ファイル（問題の画像）をコピー
fn copy_attachments(items: &[ProjectItem], attachments_dir: &Path, dest: &Path) {
    for item in items {
        for att in &item.snap_attachments {
            let src = attachments_dir.join(&att.stored_name);
            if src.exists() {
                std::fs::copy(&src, dest.join(&att.stored_name)).ok();
            }
        }
    }
}

/// スナップショットの部品添付ファイルをコピー
fn copy_part_attachments(items: &[ProjectItem], part_attachments_dir: &Path, dest: &Path) {
    for item in items {
        for att in &item.snap_part_attachments {
            let src = part_attachments_dir.join(&att.stored_name);
            if src.exists() {
                std::fs::copy(&src, dest.join(&att.stored_name)).ok();
            }
        }
    }
}

/// テンプレートアセット（画像・sty等）をコピー
pub fn copy_template_assets(assets: &[SnapAttachment], data_dir: &Path, dest: &Path) {
    let root = data_dir.join("template_assets");
    for a in assets {
        let src = root.join(&a.stored_name);
        if src.exists() {
            std::fs::copy(&src, dest.join(&a.file_name)).ok();
        }
    }
}

fn managed_graph_reference(reference: &str) -> Option<PathBuf> {
    const PREFIX: &str = "assets/graphs/";
    if !reference.starts_with(PREFIX) {
        return None;
    }
    let mut relative = PathBuf::new();
    for segment in reference[PREFIX.len()..].split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            return None;
        }
        relative.push(segment);
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// 教材のLaTeXが実際に参照しているグラフ画像だけを作業・出力フォルダーへコピーする。
/// graph_assets全体には履歴や未使用グラフも蓄積されるため、全件コピーするとPDF生成のたびに
/// 数百ファイルのI/Oとextractbb処理が発生する。
fn copy_graph_assets(data_dir: &Path, dest: &Path, tex: &str) {
    let source_root = data_dir.join("graph_assets");
    let target_root = dest.join("assets").join("graphs");
    for reference in includegraphics_references(tex) {
        let Some(relative) = managed_graph_reference(&reference) else {
            continue;
        };
        let source = source_root.join(&relative);
        if !source.is_file() {
            continue;
        }
        let target = target_root.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(source, target).ok();
    }
}

/// uplatex / dvipdfmx のパスを解決する。Err はユーザー向けメッセージ
pub fn resolve_tex_pair(conn: &rusqlite::Connection) -> Result<(PathBuf, PathBuf), String> {
    let uplatex = resolve_tex_cmd(conn, "uplatex", "uplatex_path").ok_or_else(|| {
        "uplatex が見つかりません。TeX Live または MiKTeX をインストールし、設定画面でパスを指定してください。".to_string()
    })?;
    let dvipdfmx = resolve_tex_cmd(conn, "dvipdfmx", "dvipdfmx_path")
        .ok_or_else(|| "dvipdfmx が見つかりません。設定画面でパスを指定してください。".to_string())?;
    Ok((uplatex, dvipdfmx))
}

/// 外部コマンドをタイムアウト付きで実行し、(成功, 出力) を返す。
/// 標準出力・標準エラーはファイルへリダイレクトする（パイプ詰まり防止）
fn run_logged_with_timeout(
    cmd: &mut Command,
    build_dir: &Path,
    tag: &str,
    timeout_secs: u64,
) -> Result<(bool, String), String> {
    let out_path = build_dir.join(format!("{}.stdout.txt", tag));
    let out_file = std::fs::File::create(&out_path).map_err(err_str)?;
    let err_file = out_file.try_clone().map_err(err_str)?;
    let mut child = no_window(cmd.stdout(out_file).stderr(err_file))
        .spawn()
        .map_err(|e| format!("{} の実行に失敗しました: {}", tag, e))?;
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait().map_err(err_str)? {
            Some(st) => break st,
            None => {
                let output_too_large = std::fs::metadata(&out_path)
                    .map(|m| m.len() > MAX_COMPILE_LOG_BYTES)
                    .unwrap_or(false);
                let dir_too_large = std::fs::read_dir(build_dir)
                    .map(|entries| {
                        entries
                            .filter_map(|entry| entry.ok())
                            .filter_map(|entry| entry.metadata().ok())
                            .map(|meta| meta.len())
                            .sum::<u64>()
                            > MAX_COMPILE_DIR_BYTES
                    })
                    .unwrap_or(false);
                if output_too_large || dir_too_large {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} の出力が安全上限を超えたため停止しました",
                        tag
                    ));
                }
                if start.elapsed().as_secs() > timeout_secs {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} が{}秒でタイムアウトしました", tag, timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(&out_path) {
        let _ = file
            .take(MAX_COMPILE_LOG_BYTES)
            .read_to_string(&mut text);
    }
    Ok((status.success(), text))
}

/// dvipdfmx向け画像の寸法情報を安全に事前生成する。
///
/// uplatexのgraphicxはPDF/PNG/JPEG等に対応する`.xbb`が無い場合、TeX内部から
/// `extractbb`をpipe実行しようとする。shell-escapeは有効化せず、教材工房が
/// build_dir内の実在ファイルだけを固定引数でextractbbへ渡す。
fn normalized_graphics_path(value: &Path) -> String {
    value
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_ascii_lowercase()
}

fn includegraphics_references(tex: &str) -> HashSet<String> {
    const COMMAND: &str = "\\includegraphics";
    let mut references = HashSet::new();
    let mut rest = tex;
    while let Some(position) = rest.find(COMMAND) {
        rest = &rest[position + COMMAND.len()..];
        let mut value = rest.trim_start();
        if let Some(after_star) = value.strip_prefix('*') {
            value = after_star.trim_start();
        }
        if let Some(after_open) = value.strip_prefix('[') {
            let Some(end) = after_open.find(']') else {
                continue;
            };
            value = after_open[end + 1..].trim_start();
        }
        let Some(after_open) = value.strip_prefix('{') else {
            continue;
        };
        let Some(end) = after_open.find('}') else {
            continue;
        };
        let path = after_open[..end].trim();
        if !path.is_empty() {
            references.insert(normalized_graphics_path(Path::new(path)));
        }
        rest = &after_open[end + 1..];
    }
    references
}

fn graphics_source_priority(path: &Path) -> u8 {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => 0,
        "png" => 1,
        "jpg" | "jpeg" => 2,
        "jp2" => 3,
        "bmp" => 4,
        _ => 5,
    }
}

/// `.xbb`は拡張子を持たない同名ファイルで共有されるため、`graph.pdf`と
/// `graph.png`を両方extractbbへ渡すと後から処理した側が寸法を上書きする。
/// LaTeXが実際に参照している形式を優先し、参照が不明な場合だけPDF優先で決定する。
fn select_graphics_bounding_box_sources(
    files: Vec<PathBuf>,
    tex: &str,
) -> Result<Vec<PathBuf>, String> {
    let references = includegraphics_references(tex);
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for file in files {
        let mut stem = file.clone();
        stem.set_extension("");
        groups.entry(stem).or_default().push(file);
    }

    let mut selected = Vec::with_capacity(groups.len());
    for (_, mut candidates) in groups {
        candidates.sort_by(|left, right| {
            graphics_source_priority(left)
                .cmp(&graphics_source_priority(right))
                .then_with(|| normalized_graphics_path(left).cmp(&normalized_graphics_path(right)))
        });
        let referenced: Vec<&PathBuf> = candidates
            .iter()
            .filter(|candidate| references.contains(&normalized_graphics_path(candidate)))
            .collect();
        if referenced.len() > 1 {
            return Err(format!(
                "同名で形式の異なる画像を同時に参照できません: {}",
                candidates
                    .iter()
                    .map(|value| value.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        selected.push(
            referenced
                .first()
                .copied()
                .cloned()
                .unwrap_or_else(|| candidates[0].clone()),
        );
    }
    Ok(selected)
}

fn prepare_graphics_bounding_boxes(
    dvipdfmx: &Path,
    build_dir: &Path,
    tex: &str,
) -> Result<(), String> {
    const MAX_GRAPHICS: usize = 128;
    fn collect(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(err_str)? {
            let entry = entry.map_err(err_str)?;
            let file_type = entry.file_type().map_err(err_str)?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                collect(&path, root, files)?;
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "pdf" | "png" | "jpg" | "jpeg" | "jp2" | "bmp") {
                continue;
            }
            let mut xbb = path.clone();
            xbb.set_extension("xbb");
            if !xbb.exists() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| "画像ファイルが一時作業フォルダ外にあります".to_string())?;
                files.push(relative.to_path_buf());
                if files.len() > MAX_GRAPHICS {
                    return Err(format!(
                        "画像が多すぎるためコンパイルを停止しました（上限{}件）",
                        MAX_GRAPHICS
                    ));
                }
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(build_dir, build_dir, &mut files)?;
    let files = select_graphics_bounding_box_sources(files, tex)?;
    if files.is_empty() {
        return Ok(());
    }

    let parent = dvipdfmx
        .parent()
        .ok_or_else(|| "dvipdfmxのフォルダを特定できません".to_string())?;
    let extractbb = if cfg!(windows) {
        parent.join("extractbb.exe")
    } else {
        parent.join("extractbb")
    };
    if !extractbb.is_file() {
        return Err(format!(
            "PDF・画像の寸法取得に必要なextractbbが見つかりません: {}",
            extractbb.display()
        ));
    }

    // extractbbは複数ファイルを同時に渡すと最後の1件しか処理しない実装があるため、
    // 画像ごとに起動する。全体の処理時間は60秒に制限する。
    let started = std::time::Instant::now();
    for file in &files {
        let elapsed = started.elapsed().as_secs();
        if elapsed >= 60 {
            return Err("画像の寸法情報生成がタイムアウトしました（上限60秒）".to_string());
        }
        let remaining = 60_u64.saturating_sub(elapsed).max(1);
        let (success, output) = run_logged_with_timeout(
            Command::new(&extractbb)
                .current_dir(build_dir)
                .args(["-q", "-x"])
                .arg(file),
            build_dir,
            "extractbb",
            remaining,
        )?;
        if !success {
            return Err(format!(
                "PDF・画像の寸法情報を生成できませんでした: {}\n{}",
                file.display(),
                output
            ));
        }
    }
    for file in files {
        let mut xbb = build_dir.join(file);
        xbb.set_extension("xbb");
        if !xbb.is_file() {
            return Err(format!(
                "画像の寸法情報が生成されませんでした: {}",
                xbb.display()
            ));
        }
    }
    Ok(())
}

/// uplatex → dvipdfmx を実行する（DBロック不要）。成功時は build_dir 内のPDFパスを返す
pub fn run_compile_with(
    uplatex: &Path,
    dvipdfmx: &Path,
    build_dir: &Path,
    tex: &str,
) -> Result<(bool, Option<PathBuf>, String, String), String> {
    let _compile_guard = compile_lock()
        .lock()
        .map_err(|_| "コンパイル制御ロックが壊れています".to_string())?;
    if tex.len() > MAX_TEX_BYTES {
        return Err(format!(
            "LaTeXソースが大きすぎます（上限{}MB）",
            MAX_TEX_BYTES / 1024 / 1024
        ));
    }
    let job = "kyozai";
    std::fs::create_dir_all(build_dir).map_err(|e| format!("一時フォルダの作成に失敗: {}", e))?;
    std::fs::write(build_dir.join(format!("{}.tex", job)), tex)
        .map_err(|e| format!(".texの書き込みに失敗: {}", e))?;

    if let Err(message) = prepare_graphics_bounding_boxes(dvipdfmx, build_dir, tex) {
        return Ok((false, None, message.clone(), message));
    }

    // 目次や相互参照のため2回実行する
    let mut up_success = true;
    let mut up_stdout = String::new();
    for _ in 0..2 {
        let (success, out) = run_logged_with_timeout(
            Command::new(uplatex)
                .current_dir(build_dir)
                .env("openin_any", "p")
                .env("openout_any", "p")
                .args([
                    "-no-shell-escape",
                    "-interaction=nonstopmode",
                    "-halt-on-error",
                    "-file-line-error",
                ])
                .arg(format!("{}.tex", job)),
            build_dir,
            "uplatex",
            120,
        )?;
        up_success = success;
        up_stdout = out;
        if !success {
            break;
        }
    }

    let mut log = up_stdout;
    if let Ok(file) = std::fs::File::open(build_dir.join(format!("{}.log", job))) {
        let mut bounded = String::new();
        let _ = file
            .take(MAX_COMPILE_LOG_BYTES)
            .read_to_string(&mut bounded);
        if !bounded.is_empty() {
            log = bounded;
        }
    }

    if !up_success {
        let err_lines: Vec<&str> = log
            .lines()
            .filter(|l| l.starts_with('!') || l.contains(".tex:"))
            .take(8)
            .collect();
        let msg = if err_lines.is_empty() {
            "uplatex でエラーが発生しました。ログを確認してください。".to_string()
        } else {
            format!("LaTeXコンパイルエラー:\n{}", err_lines.join("\n"))
        };
        return Ok((false, None, log, msg));
    }

    let (dvi_success, dvi_out) = run_logged_with_timeout(
        Command::new(dvipdfmx)
            .current_dir(build_dir)
            .env("openin_any", "p")
            .env("openout_any", "p")
            .arg(format!("{}.dvi", job)),
        build_dir,
        "dvipdfmx",
        120,
    )?;
    log.push_str("\n\n===== dvipdfmx =====\n");
    log.push_str(&dvi_out);

    let pdf = build_dir.join(format!("{}.pdf", job));
    if !dvi_success || !pdf.exists() {
        return Ok((false, None, log, "dvipdfmx でPDF生成に失敗しました。ログを確認してください。".into()));
    }
    if std::fs::metadata(&pdf)
        .map(|m| m.len() > MAX_PDF_BYTES)
        .unwrap_or(false)
    {
        std::fs::remove_file(&pdf).ok();
        return Ok((
            false,
            None,
            log,
            format!(
                "生成PDFが安全上限（{}MB）を超えました",
                MAX_PDF_BYTES / 1024 / 1024
            ),
        ));
    }
    Ok((true, Some(pdf), log, "PDFを生成しました".into()))
}

/// .tex ソースを文字列として返す（プレビュー用）
pub fn generate_tex(state: &AppState, project_id: i64, kind: String) -> Result<String, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let data = load_project_data(&conn, project_id)?;
    Ok(build_project_tex(&data, &kind))
}

/// .tex（と画像）を出力フォルダへ書き出す
pub fn export_tex(state: &AppState, project_id: i64, kind: String) -> Result<String, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let data = load_project_data(&conn, project_id)?;
    let tex = build_project_tex(&data, &kind);
    let out_dir = output_dir(&conn, state);
    let suffix = booklet_suffix(&kind);
    let base = sanitize_filename(&format!("{}_{}", data.name, suffix));
    let tex_path = out_dir.join(format!("{}.tex", base));
    std::fs::write(&tex_path, &tex).map_err(|e| format!(".texの書き込みに失敗しました: {}", e))?;
    copy_attachments(&data.items, &state.attachments_dir(), &out_dir);
    copy_part_attachments(&data.items, &state.part_attachments_dir(), &out_dir);
    copy_template_assets(&data.tpl.assets, &state.data_dir, &out_dir);
    copy_graph_assets(&state.data_dir, &out_dir, &tex);
    Ok(tex_path.to_string_lossy().to_string())
}

/// プロジェクトの冊子PDFを生成する
pub fn compile_pdf(state: &AppState, project_id: i64, kind: String) -> Result<CompileResult, String> {
    // DBが必要な情報を先に読み、TeX実行中はロックを保持しない（他端末の操作をブロックしないため）
    let (data, tex, tex_pair, out_dir) = {
        let conn = state.conn.lock().map_err(err_str)?;
        let data = load_project_data(&conn, project_id)?;
        let tex = build_project_tex(&data, &kind);
        let tex_pair = resolve_tex_pair(&conn);
        let out_dir = output_dir(&conn, state);
        (data, tex, tex_pair, out_dir)
    };

    let build_dir = std::env::temp_dir()
        .join("kyozai-kobo-build")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&build_dir).map_err(err_str)?;
    copy_attachments(&data.items, &state.attachments_dir(), &build_dir);
    copy_part_attachments(&data.items, &state.part_attachments_dir(), &build_dir);
    copy_template_assets(&data.tpl.assets, &state.data_dir, &build_dir);
    copy_graph_assets(&state.data_dir, &build_dir, &tex);

    let (success, pdf, log, message) = match &tex_pair {
        Ok((uplatex, dvipdfmx)) => run_compile_with(uplatex, dvipdfmx, &build_dir, &tex)?,
        Err(msg) => (false, None, String::new(), msg.clone()),
    };
    let tex_path = build_dir.join("kyozai.tex");

    if !success {
        return Ok(CompileResult {
            success: false,
            pdf_path: None,
            tex_path: Some(tex_path.to_string_lossy().to_string()),
            log,
            message,
        });
    }

    let suffix = booklet_suffix(&kind);
    let base = sanitize_filename(&format!("{}_{}", data.name, suffix));
    let dest_pdf = out_dir.join(format!("{}.pdf", base));
    let dest_tex = out_dir.join(format!("{}.tex", base));
    std::fs::copy(pdf.unwrap(), &dest_pdf)
        .map_err(|e| format!("PDFのコピーに失敗しました（開いたまま？）: {}", e))?;
    std::fs::copy(&tex_path, &dest_tex).ok();
    copy_attachments(&data.items, &state.attachments_dir(), &out_dir);
    copy_part_attachments(&data.items, &state.part_attachments_dir(), &out_dir);
    copy_template_assets(&data.tpl.assets, &state.data_dir, &out_dir);
    copy_graph_assets(&state.data_dir, &out_dir, &tex);

    Ok(CompileResult {
        success: true,
        pdf_path: Some(dest_pdf.to_string_lossy().to_string()),
        tex_path: Some(dest_tex.to_string_lossy().to_string()),
        log,
        message,
    })
}

/// サンプル教材データを返す（テンプレートのテストコンパイル用）
fn sample_items() -> Vec<ProjectItem> {
    let mk = |title: &str, statement: &str, answer: &str, explanation: &str| ProjectItem {
        id: 0,
        project_id: 0,
        item_type: "problem".into(),
        sort_order: 0,
        problem_id: None,
        part_id: None,
        snap_title: title.into(),
        snap_statement: statement.into(),
        snap_answer: answer.into(),
        snap_explanation: explanation.into(),
        snap_difficulty: "標準".into(),
        snap_difficulty_rank: Some("B".into()),
        snap_is_required: false,
        snap_attachments: vec![],
        content: String::new(),
        snap_part_type: String::new(),
        snap_part_category: String::new(),
        snap_part_description: String::new(),
        snap_part_output_target: "both".into(),
        snap_part_attachments: vec![],
        heading_level: 1,
        heading_numbered: true,
        bank_updated: false,
        source_exists: true,
        part_updated: false,
        version: 1,
    };
    vec![
        mk(
            "サンプル問題1",
            "二次関数 $y = x^2 - 4x + 7$ を $y = a(x-p)^2 + q$ の形に変形し、頂点の座標を求めよ。",
            "$y = (x-2)^2 + 3$ より頂点は $(2,\\ 3)$。",
            "平方完成を行う。$x^2-4x = (x-2)^2 - 4$ の変形がポイント。",
        ),
        mk(
            "サンプル問題2",
            "二次方程式 $x^2 + 2kx + k + 2 = 0$ が異なる2つの実数解をもつとき、定数 $k$ の値の範囲を求めよ。",
            "\\[ \\frac{D}{4} = k^2 - k - 2 = (k-2)(k+1) > 0 \\] より $k < -1,\\ 2 < k$。",
            "$D/4$ を使うと計算が簡単になる。",
        ),
    ]
}

fn sample_settings() -> ProjectSettings {
    ProjectSettings {
        booklet_title: "テンプレートプレビュー".into(),
        subtitle: "サンプル教材データ".into(),
        target: "高1".into(),
        date_str: chrono::Local::now().format("%Y年%m月%d日").to_string(),
        header_left: String::new(),
        header_right: String::new(),
        number_format: "問題{n}".into(),
        show_name_field: true,
        auto_number: true,
        page_break_per_problem: false,
        include_explanation: true,
        two_column_mode: "none".into(),
        show_title: true,
        show_header: true,
        show_toc: false,
        number_headings: false,
        include_statement_in_answers: true,
        box_statement_in_answers: false,
        reset_numbering_per_chapter: true,
        difficulty_display: "number_side".into(),
        required_display: "required_only".into(),
    }
}

/// テンプレートをサンプルデータでテストコンパイルする
pub fn test_compile_template(
    state: &AppState,
    template_id: i64,
    kind: String,
) -> Result<CompileResult, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (base, problem, answer): (String, String, String) = conn
        .query_row(
            "SELECT base_template, problem_template, answer_template FROM templates WHERE id=?1",
            params![template_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(err_str)?;
    let tpl = if kind == "answers" {
        if !answer.trim().is_empty() { answer } else if !base.trim().is_empty() { base } else { DEFAULT_ANSWER_TEMPLATE.into() }
    } else if !problem.trim().is_empty() {
        problem
    } else if !base.trim().is_empty() {
        base
    } else {
        DEFAULT_PROBLEM_TEMPLATE.into()
    };

    let items = sample_items();
    let settings = sample_settings();
    let bodies = render_bodies(&items, &settings);
    let tex = render_document(&tpl, &kind, "テンプレートプレビュー", &settings, &bodies);

    let build_dir = std::env::temp_dir()
        .join("kyozai-kobo-build")
        .join(format!("tpl-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&build_dir).map_err(err_str)?;

    // テンプレートアセットをコピー
    let assets = {
        let mut stmt = conn
            .prepare("SELECT file_name, stored_name FROM template_assets WHERE template_id=?1")
            .map_err(err_str)?;
        let rows: Vec<SnapAttachment> = stmt
            .query_map(params![template_id], |r| {
                Ok(SnapAttachment {
                    file_name: r.get(0)?,
                    stored_name: r.get(1)?,
                })
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let tex_pair = resolve_tex_pair(&conn);
    drop(conn);
    copy_template_assets(&assets, &state.data_dir, &build_dir);

    let (success, pdf, log, message) = match &tex_pair {
        Ok((uplatex, dvipdfmx)) => run_compile_with(uplatex, dvipdfmx, &build_dir, &tex)?,
        Err(msg) => (false, None, String::new(), msg.clone()),
    };
    Ok(CompileResult {
        success,
        pdf_path: pdf.map(|p| p.to_string_lossy().to_string()),
        tex_path: Some(build_dir.join("kyozai.tex").to_string_lossy().to_string()),
        log,
        message,
    })
}

/// テンプレートのプリアンブルを使って単問プレビュー用の完全なLaTeX文書を作る。
/// テンプレートに定義された \usepackage や独自コマンドがそのまま使えるため、
/// プレビューと冊子出力でコンパイル環境が一致する。
pub fn build_preview_doc(effective_template: &str, statement: &str, answer: &str, explanation: &str) -> String {
    use super::templates::KNOWN_PLACEHOLDERS;
    // プリアンブル（\begin{document} まで）を抽出。無ければ既定テンプレートのものを使う
    let preamble_src = match effective_template.find("\\begin{document}") {
        Some(pos) => &effective_template[..pos],
        None => {
            let d = super::templates::DEFAULT_PROBLEM_TEMPLATE;
            &d[..d.find("\\begin{document}").unwrap()]
        }
    };
    let mut preamble = preamble_src.to_string();
    for ph in KNOWN_PLACEHOLDERS {
        preamble = preamble.replace(&format!("{{{{{}}}}}", ph), "");
    }

    let mut doc = preamble;
    doc.push_str("\\pagestyle{empty}\n\\begin{document}\n");
    doc.push_str(statement);
    doc.push('\n');
    if !answer.trim().is_empty() {
        doc.push_str("\\par\\vspace{0.8em}\n\\noindent\\textbf{【解答】}\\par\n");
        doc.push_str(answer);
        doc.push('\n');
    }
    if !explanation.trim().is_empty() {
        doc.push_str("\\par\\vspace{0.8em}\n\\noindent\\textbf{【解説】}\\par\n");
        doc.push_str(explanation);
        doc.push('\n');
    }
    doc.push_str("\\end{document}\n");
    doc
}

/// プレビュー・AI試験コンパイルに使うテンプレートを解決する
/// （設定「preview_template_id」→ 先頭のテンプレート → 組み込み既定）
pub fn resolve_preview_template(conn: &rusqlite::Connection) -> (Option<i64>, String) {
    let template_id: Option<i64> = get_setting(conn, "preview_template_id")
        .and_then(|v| v.parse().ok())
        .filter(|id| {
            conn.query_row("SELECT 1 FROM templates WHERE id=?1", params![id], |_| Ok(()))
                .is_ok()
        })
        .or_else(|| {
            conn.query_row("SELECT id FROM templates ORDER BY id LIMIT 1", [], |r| r.get(0))
                .ok()
        });
    let effective_tpl = match template_id {
        Some(tid) => {
            let row: Result<(String, String), _> = conn.query_row(
                "SELECT base_template, problem_template FROM templates WHERE id=?1",
                params![tid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            );
            match row {
                Ok((base, problem)) => {
                    if !problem.trim().is_empty() {
                        problem
                    } else if !base.trim().is_empty() {
                        base
                    } else {
                        super::templates::DEFAULT_PROBLEM_TEMPLATE.to_string()
                    }
                }
                Err(_) => super::templates::DEFAULT_PROBLEM_TEMPLATE.to_string(),
            }
        }
        None => super::templates::DEFAULT_PROBLEM_TEMPLATE.to_string(),
    };
    (template_id, effective_tpl)
}

/// テンプレートのアセット（.sty・画像等）一覧
pub fn template_assets_of(conn: &rusqlite::Connection, template_id: i64) -> Vec<SnapAttachment> {
    let Ok(mut stmt) =
        conn.prepare("SELECT file_name, stored_name FROM template_assets WHERE template_id=?1")
    else {
        return vec![];
    };
    stmt.query_map(params![template_id], |r| {
        Ok(SnapAttachment {
            file_name: r.get(0)?,
            stored_name: r.get(1)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// 問題1問だけをコンパイルしてプレビュー用PDFを生成する（未保存の編集内容をそのまま渡せる）。
/// プリアンブルは設定「preview_template_id」のテンプレート（未設定なら先頭のテンプレート）を使う。
pub fn compile_problem_preview(
    state: &AppState,
    problem_id: i64,
    statement: String,
    answer: String,
    explanation: String,
) -> Result<CompileResult, String> {
    let conn = state.conn.lock().map_err(err_str)?;

    let (template_id, effective_tpl) = resolve_preview_template(&conn);

    let doc = build_preview_doc(&effective_tpl, &statement, &answer, &explanation);

    let build_dir = std::env::temp_dir()
        .join("kyozai-kobo-build")
        .join(format!("preview-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&build_dir).map_err(err_str)?;

    // 添付画像をコピーして \includegraphics を解決できるようにする
    if let Ok(atts) = super::problems::attachments_of(&conn, problem_id) {
        let dir = state.attachments_dir();
        for a in atts {
            let src = dir.join(&a.stored_name);
            if src.exists() {
                std::fs::copy(&src, build_dir.join(&a.stored_name)).ok();
            }
        }
    }
    // テンプレートのアセット（.sty・画像等）もコピー
    if let Some(tid) = template_id {
        let assets: Vec<SnapAttachment> = {
            let mut stmt = conn
                .prepare("SELECT file_name, stored_name FROM template_assets WHERE template_id=?1")
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![tid], |r| {
                    Ok(SnapAttachment {
                        file_name: r.get(0)?,
                        stored_name: r.get(1)?,
                    })
                })
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        };
        copy_template_assets(&assets, &state.data_dir, &build_dir);
    }
    copy_graph_assets(&state.data_dir, &build_dir, &doc);

    let tex_pair = resolve_tex_pair(&conn);
    drop(conn);
    let (success, pdf, log, message) = match &tex_pair {
        Ok((uplatex, dvipdfmx)) => run_compile_with(uplatex, dvipdfmx, &build_dir, &doc)?,
        Err(msg) => (false, None, String::new(), msg.clone()),
    };
    Ok(CompileResult {
        success,
        pdf_path: pdf.map(|p| p.to_string_lossy().to_string()),
        tex_path: Some(build_dir.join("kyozai.tex").to_string_lossy().to_string()),
        log,
        message,
    })
}

/// 生成済みのコンパイル成果物をbase64で返す（デスクトップのPDFプレビュー用）。
/// WebView（tauri.localhost）からasset protocol（asset.localhost）へのfetchは
/// クロスオリジンとなりCORSで失敗するため、IPC経由でバイト列を渡して
/// フロント側でblob URLにして表示する。許可ルートはWebの/api/files/buildと同一。
pub fn read_compiled_file(state: &AppState, path: String) -> Result<String, String> {
    use base64::Engine;
    let canonical = crate::server::resolve_compiled_file(state, Path::new(&path))
        .map_err(|e| e.message().to_string())?;
    let meta = std::fs::metadata(&canonical).map_err(err_str)?;
    if !meta.is_file() {
        return Err("ファイルが見つかりません".into());
    }
    if meta.len() > MAX_PDF_BYTES {
        return Err("ファイルが大きすぎます".into());
    }
    let bytes = std::fs::read(&canonical).map_err(err_str)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
pub fn open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !Path::new(&path).exists() {
        return Err("ファイルが見つかりません".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(err_str)?;
    Ok(())
}

#[tauri::command]
pub fn show_in_folder(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(err_str)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_graph_reference_rejects_traversal() {
        assert_eq!(
            managed_graph_reference("assets/graphs/snapshots/graphasset_1/graph.pdf"),
            Some(PathBuf::from("snapshots/graphasset_1/graph.pdf"))
        );
        assert!(managed_graph_reference("assets/graphs/../attachments/secret.pdf").is_none());
        assert!(managed_graph_reference("attachments/graph.pdf").is_none());
    }

    #[test]
    fn graph_copy_only_includes_latex_references() {
        let root = tempdir::TempDir::new("graph-copy-source").unwrap();
        let output = tempdir::TempDir::new("graph-copy-output").unwrap();
        let graph_a = root.path().join("graph_assets/snapshots/graphasset_a");
        let graph_b = root.path().join("graph_assets/snapshots/graphasset_b");
        std::fs::create_dir_all(&graph_a).unwrap();
        std::fs::create_dir_all(&graph_b).unwrap();
        std::fs::write(graph_a.join("graph.pdf"), b"pdf-a").unwrap();
        std::fs::write(graph_a.join("graph.png"), b"png-a").unwrap();
        std::fs::write(graph_b.join("graph.pdf"), b"pdf-b").unwrap();

        copy_graph_assets(
            root.path(),
            output.path(),
            r"\includegraphics{assets/graphs/snapshots/graphasset_a/graph.pdf}",
        );

        let copied = output.path().join("assets/graphs/snapshots");
        assert!(copied.join("graphasset_a/graph.pdf").is_file());
        assert!(!copied.join("graphasset_a/graph.png").exists());
        assert!(!copied.join("graphasset_b/graph.pdf").exists());
    }
}
