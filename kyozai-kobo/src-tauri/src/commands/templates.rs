use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// 初期テンプレート「高校数学教材・標準」問題冊子用
pub const DEFAULT_PROBLEM_TEMPLATE: &str = r#"\documentclass[uplatex,a4paper,11pt]{ujarticle}

\usepackage[dvipdfmx]{graphicx}
\usepackage{amsmath,amssymb,mathtools}
\usepackage{geometry}
\usepackage{fancyhdr}
\usepackage{titlesec}
\usepackage{ascmac}
\usepackage{multicol}
\usepackage{tcolorbox}
\tcbuselibrary{skins}
\usepackage{enumitem}
\usepackage{physics}
\usepackage{float}

\geometry{
  top=20mm,
  bottom=25mm,
  left=20mm,
  right=20mm
}

\pagestyle{fancy}
\fancyhf{}
\lhead{{{HEADER_LEFT}}}
\rhead{{{HEADER_RIGHT}}}
\cfoot{\thepage}
\setlength{\headheight}{15pt}

\newcounter{mondai}
\newcommand{\mondai}{%
  \refstepcounter{mondai}%
  \par\medskip
  \noindent\textbf{問題\themondai}\par
}

\begin{document}

\begin{center}
  {\LARGE \bfseries {{TITLE}} \par}
  \vspace{2mm}
  {\large {{SUBTITLE}} \par}
\end{center}

{{NAME_FIELD}}

{{BODY}}

\end{document}
"#;

/// 初期テンプレート「高校数学教材・標準」解答冊子用
pub const DEFAULT_ANSWER_TEMPLATE: &str = r#"\documentclass[uplatex,a4paper,11pt]{ujarticle}

\usepackage[dvipdfmx]{graphicx}
\usepackage{amsmath,amssymb,mathtools}
\usepackage{geometry}
\usepackage{fancyhdr}
\usepackage{titlesec}
\usepackage{ascmac}
\usepackage{multicol}
\usepackage{tcolorbox}
\tcbuselibrary{skins}
\usepackage{enumitem}
\usepackage{physics}
\usepackage{float}

\geometry{
  top=20mm,
  bottom=25mm,
  left=20mm,
  right=20mm
}

\pagestyle{fancy}
\fancyhf{}
\lhead{{{HEADER_LEFT}}}
\rhead{{{HEADER_RIGHT}}}
\cfoot{\thepage}
\setlength{\headheight}{15pt}

\begin{document}

\begin{center}
  {\LARGE \bfseries {{ANSWER_TITLE}} \par}
  \vspace{2mm}
  {\large {{SUBTITLE}} \par}
\end{center}

{{NAME_FIELD}}

{{ANSWER_BODY}}

\end{document}
"#;

pub const KNOWN_PLACEHOLDERS: [&str; 13] = [
    "TITLE",
    "ANSWER_TITLE",
    "SUBTITLE",
    "TARGET",
    "DATE",
    "NAME_FIELD",
    "HEADER_LEFT",
    "HEADER_RIGHT",
    "BODY",
    "ANSWER_BODY",
    "EXPLANATION_BODY",
    "PAGE_BREAK",
    "TOC",
];

pub const MARKER_START: &str = "% APP_BODY_START";
pub const MARKER_END: &str = "% APP_BODY_END";

/// テンプレートの構文チェック。警告メッセージの一覧を返す
pub fn validate_templates(base: &str, problem: &str, answer: &str) -> Vec<String> {
    let mut warnings = vec![];
    let effective_problem = if problem.trim().is_empty() { base } else { problem };
    let effective_answer = if answer.trim().is_empty() { base } else { answer };

    if effective_problem.trim().is_empty() {
        warnings.push("問題冊子用テンプレートが空です（共通テンプレートも未設定）。".into());
    } else {
        if !effective_problem.contains("{{BODY}}")
            && !(effective_problem.contains(MARKER_START) && effective_problem.contains(MARKER_END))
        {
            warnings.push(
                "問題冊子テンプレートに {{BODY}} または % APP_BODY_START / % APP_BODY_END がありません。問題本文が挿入されません。".into(),
            );
        }
        if !effective_problem.contains("\\begin{document}") {
            warnings.push("問題冊子テンプレートに \\begin{document} が見つかりません。".into());
        }
        if effective_problem.contains("{{BODY}}")
            && effective_problem.contains(MARKER_START)
            && effective_problem.contains(MARKER_END)
        {
            warnings.push(
                "問題冊子テンプレートに {{BODY}} と % APP_BODY_START / % APP_BODY_END の両方があります。本文が二重に挿入されます。".into(),
            );
        }
    }
    if effective_answer.trim().is_empty() {
        warnings.push("解答冊子用テンプレートが空です（共通テンプレートも未設定）。".into());
    } else if !effective_answer.contains("{{ANSWER_BODY}}")
        && !effective_answer.contains("{{BODY}}")
        && !(effective_answer.contains(MARKER_START) && effective_answer.contains(MARKER_END))
    {
        warnings.push(
            "解答冊子テンプレートに {{ANSWER_BODY}}（または {{BODY}} / APP_BODYマーカー）がありません。解答本文が挿入されません。".into(),
        );
    } else if (effective_answer.contains("{{ANSWER_BODY}}") || effective_answer.contains("{{BODY}}"))
        && effective_answer.contains(MARKER_START)
        && effective_answer.contains(MARKER_END)
    {
        warnings.push(
            "解答冊子テンプレートに本文プレースホルダと % APP_BODY_START / % APP_BODY_END の両方があります。解答本文が二重に挿入されます。".into(),
        );
    }

    // 不明なプレースホルダの検出
    for tpl in [base, problem, answer] {
        let mut rest = tpl;
        while let Some(pos) = rest.find("{{") {
            let after = &rest[pos + 2..];
            if let Some(end) = after.find("}}") {
                let name = &after[..end];
                if !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                    && !KNOWN_PLACEHOLDERS.contains(&name)
                {
                    let w = format!("不明なプレースホルダ {{{{{}}}}} があります（置換されません）。", name);
                    if !warnings.contains(&w) {
                        warnings.push(w);
                    }
                }
                rest = &after[end + 2..];
            } else {
                break;
            }
        }
    }
    warnings
}

fn assets_of(conn: &Connection, template_id: i64) -> rusqlite::Result<Vec<TemplateAsset>> {
    let mut stmt = conn.prepare(
        "SELECT id, template_id, file_name, stored_name FROM template_assets WHERE template_id=?1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![template_id], |r| {
            Ok(TemplateAsset {
                id: r.get(0)?,
                template_id: r.get(1)?,
                file_name: r.get(2)?,
                stored_name: r.get(3)?,
            })
        })?
        .collect();
    rows
}

/// 初期データ: テンプレートが1件も無ければ標準テンプレートを作成し、既存プロジェクトに紐付ける。
/// 一度も編集されていない既定テンプレートは、新しい既定内容へ自動更新する。
pub fn seed_default_template(conn: &Connection) -> rusqlite::Result<()> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM templates", [], |r| r.get(0))?;
    if n > 0 {
        // 未編集（履歴なし）の既定テンプレートに新パッケージ（physics/float等）を反映
        conn.execute(
            "UPDATE templates SET problem_template=?1, answer_template=?2,
                    packages_memo=?3, updated_at=?4
             WHERE name='高校数学教材・標準'
               AND problem_template NOT LIKE '%physics%'
               AND id NOT IN (SELECT DISTINCT template_id FROM template_versions)",
            rusqlite::params![
                DEFAULT_PROBLEM_TEMPLATE,
                DEFAULT_ANSWER_TEMPLATE,
                "amsmath, amssymb, mathtools, graphicx, geometry, fancyhdr, titlesec, ascmac, multicol, tcolorbox(skins), enumitem, physics, float",
                now_str()
            ],
        )?;
        return Ok(());
    }
    let now = now_str();
    conn.execute(
        "INSERT INTO templates (name, description, problem_template, answer_template, compile_method, packages_memo, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'uplatex+dvipdfmx', ?5, ?6, ?6)",
        params![
            "高校数学教材・標準",
            "A4・白黒印刷向けの標準教材テンプレート（ujarticle / uplatex + dvipdfmx）",
            DEFAULT_PROBLEM_TEMPLATE,
            DEFAULT_ANSWER_TEMPLATE,
            "amsmath, amssymb, mathtools, graphicx, geometry, fancyhdr, titlesec, ascmac, multicol, tcolorbox(skins), enumitem, physics, float",
            now
        ],
    )?;
    let tid = conn.last_insert_rowid();
    // テンプレート未設定の既存プロジェクトへスナップショット付きで紐付け
    conn.execute(
        "UPDATE projects SET template_id=?1, snap_tpl_name=?2, snap_tpl_problem=?3, snap_tpl_answer=?4, snap_tpl_compile='uplatex+dvipdfmx'
         WHERE template_id IS NULL AND snap_tpl_problem=''",
        params![tid, "高校数学教材・標準", DEFAULT_PROBLEM_TEMPLATE, DEFAULT_ANSWER_TEMPLATE],
    )?;
    Ok(())
}

pub fn list_templates(state: &AppState) -> Result<Vec<TemplateSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.name, t.description, t.compile_method, t.updated_at,
                    (SELECT COUNT(*) FROM projects p WHERE p.template_id = t.id)
             FROM templates t ORDER BY t.id",
        )
        .map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TemplateSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                compile_method: r.get(3)?,
                updated_at: r.get(4)?,
                usage_count: r.get(5)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

fn template_row(conn: &Connection, id: i64) -> rusqlite::Result<TemplateFull> {
    conn.query_row(
        "SELECT id, name, description, base_template, problem_template, answer_template, compile_method, packages_memo, created_at, updated_at, version
         FROM templates WHERE id=?1",
        params![id],
        |r| {
            Ok(TemplateFull {
                id: r.get(0)?,
                version: r.get(10)?,
                name: r.get(1)?,
                description: r.get(2)?,
                base_template: r.get(3)?,
                problem_template: r.get(4)?,
                answer_template: r.get(5)?,
                compile_method: r.get(6)?,
                packages_memo: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
                assets: vec![],
                warnings: vec![],
            })
        },
    )
}

pub fn get_template(state: &AppState, id: i64) -> Result<TemplateFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut t = template_row(&conn, id).map_err(err_str)?;
    t.assets = assets_of(&conn, id).map_err(err_str)?;
    t.warnings = validate_templates(&t.base_template, &t.problem_template, &t.answer_template);
    Ok(t)
}

pub fn create_template(state: &AppState, name: String) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    let name = if name.trim().is_empty() { "新しいテンプレート".to_string() } else { name.trim().to_string() };
    conn.execute(
        "INSERT INTO templates (name, problem_template, answer_template, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![name, DEFAULT_PROBLEM_TEMPLATE, DEFAULT_ANSWER_TEMPLATE, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

fn save_template_version(conn: &Connection, template_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO template_versions (template_id, name, description, base_template, problem_template, answer_template, compile_method, packages_memo, saved_at)
         SELECT id, name, description, base_template, problem_template, answer_template, compile_method, packages_memo, ?2 FROM templates WHERE id=?1",
        params![template_id, now_str()],
    )?;
    conn.execute(
        "DELETE FROM template_versions WHERE template_id=?1 AND id NOT IN (
            SELECT id FROM template_versions WHERE template_id=?1 ORDER BY id DESC LIMIT 30)",
        params![template_id],
    )?;
    Ok(())
}

/// テンプレートを更新し、検証警告を返す
pub fn update_template(state: &AppState, payload: TemplateUpdate) -> Result<Vec<String>, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let current: i64 = tx
        .query_row("SELECT version FROM templates WHERE id=?1", params![payload.id], |r| r.get(0))
        .map_err(err_str)?;
    if payload.expected_version.is_some_and(|expected| expected != current) {
        return Err(format!("CONFLICT:{}", current));
    }
    save_template_version(&tx, payload.id).map_err(err_str)?;
    tx.execute(
        "UPDATE templates SET name=?1, description=?2, base_template=?3, problem_template=?4, answer_template=?5, compile_method=?6, packages_memo=?7, updated_at=?8, version=version+1 WHERE id=?9",
        params![
            payload.name,
            payload.description,
            payload.base_template,
            payload.problem_template,
            payload.answer_template,
            payload.compile_method,
            payload.packages_memo,
            now_str(),
            payload.id
        ],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(validate_templates(&payload.base_template, &payload.problem_template, &payload.answer_template))
}

pub fn delete_template(state: &AppState, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    // プロジェクト側はスナップショットを持つため、参照はNULLになっても再生成できる
    conn.execute("DELETE FROM templates WHERE id=?1", params![id]).map_err(err_str)?;
    Ok(())
}

pub fn duplicate_template(state: &AppState, id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO templates (name, description, base_template, problem_template, answer_template, compile_method, packages_memo, created_at, updated_at)
         SELECT name || ' (コピー)', description, base_template, problem_template, answer_template, compile_method, packages_memo, ?2, ?2
         FROM templates WHERE id=?1",
        params![id, now],
    )
    .map_err(err_str)?;
    let new_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO template_assets (template_id, file_name, stored_name, created_at)
         SELECT ?2, file_name, stored_name, ?3 FROM template_assets WHERE template_id=?1",
        params![id, new_id, now],
    )
    .map_err(err_str)?;
    Ok(new_id)
}

pub fn list_template_versions(state: &AppState, template_id: i64) -> Result<Vec<TemplateVersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT id, name, saved_at FROM template_versions WHERE template_id=?1 ORDER BY id DESC")
        .map_err(err_str)?;
    let rows = stmt
        .query_map(params![template_id], |r| {
            Ok(TemplateVersionSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                saved_at: r.get(2)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn restore_template_version(state: &AppState, version_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let template_id: i64 = tx
        .query_row("SELECT template_id FROM template_versions WHERE id=?1", params![version_id], |r| r.get(0))
        .map_err(err_str)?;
    save_template_version(&tx, template_id).map_err(err_str)?;
    tx.execute(
        "UPDATE templates SET
            name=(SELECT name FROM template_versions WHERE id=?1),
            description=(SELECT description FROM template_versions WHERE id=?1),
            base_template=(SELECT base_template FROM template_versions WHERE id=?1),
            problem_template=(SELECT problem_template FROM template_versions WHERE id=?1),
            answer_template=(SELECT answer_template FROM template_versions WHERE id=?1),
            compile_method=(SELECT compile_method FROM template_versions WHERE id=?1),
            packages_memo=(SELECT packages_memo FROM template_versions WHERE id=?1),
            updated_at=?2,
            version=version+1
         WHERE id=?3",
        params![version_id, now_str(), template_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

// ---- 既存 .tex テンプレートの取り込み ----

fn find_referenced_files(content: &str, source_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found = vec![];
    let mut push_if_exists = |name: &str| {
        let name = name.trim();
        if name.is_empty() || name.contains("{{") {
            return;
        }
        let candidates: Vec<PathBuf> = if Path::new(name).extension().is_some() {
            vec![source_dir.join(name)]
        } else {
            ["png", "jpg", "jpeg", "pdf", "eps", "sty", "tex", "cls"]
                .iter()
                .map(|e| source_dir.join(format!("{}.{}", name, e)))
                .collect()
        };
        for c in candidates {
            if c.is_file() {
                let fname = c.file_name().unwrap().to_string_lossy().to_string();
                if !found.iter().any(|(f, _): &(String, PathBuf)| f == &fname) {
                    found.push((fname, c.clone()));
                }
                break;
            }
        }
    };

    for pat in ["\\includegraphics", "\\input{", "\\include{"] {
        let mut rest = content;
        while let Some(pos) = rest.find(pat) {
            let after = &rest[pos..];
            if let Some(open) = after.find('{') {
                if let Some(close) = after[open + 1..].find('}') {
                    push_if_exists(&after[open + 1..open + 1 + close]);
                    rest = &after[open + 1 + close..];
                    continue;
                }
            }
            rest = &after[pat.len()..];
        }
    }
    found
}

fn extract_between(content: &str, start_pat: &str, open: char, close: char) -> Vec<String> {
    let mut out = vec![];
    let mut rest = content;
    while let Some(pos) = rest.find(start_pat) {
        let after = &rest[pos + start_pat.len()..];
        // オプション引数 [...] を飛ばす
        let after2 = if after.starts_with('[') {
            match after.find(']') {
                Some(i) => &after[i + 1..],
                None => after,
            }
        } else {
            after
        };
        if after2.starts_with(open) {
            if let Some(end) = after2.find(close) {
                out.push(after2[1..end].to_string());
                rest = &after2[end..];
                continue;
            }
        }
        rest = after;
    }
    out
}

/// .texファイルを解析して取り込みウィザード用の情報を返す
pub fn analyze_tex_file(state: &AppState, path: String) -> Result<ImportAnalysis, String> {
    let _ = &state;
    let src = Path::new(&path);
    let bytes = std::fs::read(src).map_err(|e| format!("ファイルを読み込めません: {}", e))?;
    let content = String::from_utf8_lossy(&bytes).to_string();
    let doc_class = extract_between(&content, "\\documentclass", '{', '}')
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut packages = vec![];
    for p in extract_between(&content, "\\usepackage", '{', '}') {
        for name in p.split(',') {
            let name = name.trim().to_string();
            if !name.is_empty() && !packages.contains(&name) {
                packages.push(name);
            }
        }
    }
    let source_dir = src.parent().unwrap_or(Path::new("."));
    let referenced_files = find_referenced_files(&content, source_dir)
        .into_iter()
        .map(|(f, _)| f)
        .collect();
    Ok(ImportAnalysis {
        doc_class,
        packages,
        has_body_placeholder: content.contains("{{BODY}}") || content.contains("{{ANSWER_BODY}}"),
        has_markers: content.contains(MARKER_START) && content.contains(MARKER_END),
        has_document_env: content.contains("\\begin{document}") && content.contains("\\end{document}"),
        referenced_files,
        content,
    })
}

/// 解析済みの .tex からテンプレートを作成する。
/// mode: "as_is"（プレースホルダ/マーカーをそのまま使う） | "replace_body"（\begin{document}〜\end{document}の中身を{{BODY}}に置換）
pub fn import_template_from_tex(
    state: &AppState,
    path: String,
    name: String,
    mode: String,
) -> Result<i64, String> {
    let src = Path::new(&path);
    let bytes = std::fs::read(src).map_err(|e| format!("ファイルを読み込めません: {}", e))?;
    let content = String::from_utf8_lossy(&bytes).to_string();

    let problem_tpl = match mode.as_str() {
        "replace_body" => {
            let begin = content
                .find("\\begin{document}")
                .ok_or("\\begin{document} が見つかりません")?;
            let end = content.find("\\end{document}").ok_or("\\end{document} が見つかりません")?;
            if end < begin {
                return Err("\\begin{document} と \\end{document} の順序が不正です".into());
            }
            let insert_at = begin + "\\begin{document}".len();
            format!(
                "{}\n\n{{{{BODY}}}}\n\n{}",
                &content[..insert_at],
                &content[end..]
            )
        }
        _ => content.clone(),
    };
    // 解答冊子用: {{BODY}} を {{ANSWER_BODY}} に置き換えたものを初期値とする
    let answer_tpl = problem_tpl.replace("{{BODY}}", "{{ANSWER_BODY}}");

    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    let name = if name.trim().is_empty() {
        src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("取り込みテンプレート".into())
    } else {
        name.trim().to_string()
    };
    conn.execute(
        "INSERT INTO templates (name, description, problem_template, answer_template, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![name, format!("{} から取り込み", src.display()), problem_tpl, answer_tpl, now],
    )
    .map_err(err_str)?;
    let tid = conn.last_insert_rowid();

    // 参照ファイル（画像・sty等）をアセットとしてコピー
    let source_dir = src.parent().unwrap_or(Path::new("."));
    let asset_dir = state.data_dir.join("template_assets").join(tid.to_string());
    std::fs::create_dir_all(&asset_dir).ok();
    for (fname, fpath) in find_referenced_files(&content, source_dir) {
        let ext = Path::new(&fname).extension().and_then(|e| e.to_str()).unwrap_or("");
        let disk_name = if ext.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            format!("{}.{}", uuid::Uuid::new_v4(), ext)
        };
        let dest = asset_dir.join(&disk_name);
        if std::fs::copy(&fpath, &dest).is_ok() {
            let stored = format!("{}/{}", tid, disk_name);
            if let Err(e) = conn.execute(
                "INSERT INTO template_assets (template_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![tid, fname, stored, now],
            ) {
                std::fs::remove_file(&dest).ok();
                return Err(err_str(e));
            }
        }
    }
    Ok(tid)
}

/// テンプレートに手動でアセット（画像・styファイル等）を追加する
pub fn add_template_asset(state: &AppState, template_id: i64, source_path: String) -> Result<TemplateAsset, String> {
    let src = Path::new(&source_path);
    if !src.is_file() {
        return Err("ファイルが見つかりません".into());
    }
    let fname = src.file_name().unwrap().to_string_lossy().to_string();
    let asset_dir = state.data_dir.join("template_assets").join(template_id.to_string());
    std::fs::create_dir_all(&asset_dir).map_err(err_str)?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
    let disk_name = if ext.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        format!("{}.{}", uuid::Uuid::new_v4(), ext)
    };
    let dest = asset_dir.join(&disk_name);
    std::fs::copy(src, &dest).map_err(err_str)?;
    let stored = format!("{}/{}", template_id, disk_name);
    let mut conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    let tx = conn.transaction().map_err(err_str)?;
    if let Err(e) = tx.execute(
        "INSERT INTO template_assets (template_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![template_id, &fname, &stored, now],
    ) {
        std::fs::remove_file(&dest).ok();
        return Err(err_str(e));
    }
    let id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE templates SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), template_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(TemplateAsset {
        id,
        template_id,
        file_name: fname,
        stored_name: stored,
    })
}

pub fn remove_template_asset(state: &AppState, asset_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let template_id: i64 = tx
        .query_row("SELECT template_id FROM template_assets WHERE id=?1", params![asset_id], |r| r.get(0))
        .map_err(err_str)?;
    // スナップショットが参照している可能性があるため実ファイルは残す
    tx.execute("DELETE FROM template_assets WHERE id=?1", params![asset_id])
        .map_err(err_str)?;
    tx.execute(
        "UPDATE templates SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), template_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

// ---- エクスポート / インポート ----

pub fn export_template(state: &AppState, id: i64, dest_path: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let t = template_row(&conn, id).map_err(err_str)?;
    let json = serde_json::json!({
        "kyozai_kobo_template": 1,
        "name": t.name,
        "description": t.description,
        "base_template": t.base_template,
        "problem_template": t.problem_template,
        "answer_template": t.answer_template,
        "compile_method": t.compile_method,
        "packages_memo": t.packages_memo,
    });
    std::fs::write(&dest_path, serde_json::to_string_pretty(&json).map_err(err_str)?)
        .map_err(|e| format!("書き込みに失敗しました: {}", e))?;
    Ok(())
}

pub fn import_template_file(state: &AppState, path: String) -> Result<i64, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("読み込みに失敗しました: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "テンプレートファイルの形式が不正です")?;
    if v.get("kyozai_kobo_template").is_none() {
        return Err("教材工房のテンプレートファイルではありません".into());
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO templates (name, description, base_template, problem_template, answer_template, compile_method, packages_memo, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            if s("name").is_empty() { "インポートしたテンプレート".to_string() } else { s("name") },
            s("description"),
            s("base_template"),
            s("problem_template"),
            s("answer_template"),
            if s("compile_method").is_empty() { "uplatex+dvipdfmx".to_string() } else { s("compile_method") },
            s("packages_memo"),
            now
        ],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}
