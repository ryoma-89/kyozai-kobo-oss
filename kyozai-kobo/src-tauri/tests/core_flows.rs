//! 主要フローの統合テスト:
//! スキーマ作成 → 階層・問題の登録 → 検索SQL → プロジェクトへのスナップショット追加 →
//! バンク側更新後もスナップショットが不変であること → .tex生成 → (TeXがあれば) PDF生成

use kyozai_kobo_lib::commands::latex::{render_bodies, render_document, run_compile_with};
use kyozai_kobo_lib::commands::projects::items_of;
use kyozai_kobo_lib::commands::templates::{
    seed_default_template, validate_templates, DEFAULT_ANSWER_TEMPLATE, DEFAULT_PROBLEM_TEMPLATE,
};
use kyozai_kobo_lib::db;
use kyozai_kobo_lib::models::{ProjectItem, ProjectSettings};
use rusqlite::params;
use std::path::PathBuf;

/// 旧APIと同等のヘルパー: テンプレートで冊子全体の .tex を生成
fn build_tex(name: &str, settings: &ProjectSettings, items: &[ProjectItem], kind: &str) -> String {
    let bodies = render_bodies(items, settings);
    let tpl = if kind == "answers" { DEFAULT_ANSWER_TEMPLATE } else { DEFAULT_PROBLEM_TEMPLATE };
    render_document(tpl, kind, name, settings, &bodies)
}

fn setup() -> (tempdir::TempDir, rusqlite::Connection) {
    let dir = tempdir::TempDir::new("kyozai-test").unwrap();
    let conn = db::open_db(dir.path()).unwrap();
    (dir, conn)
}

/// 階層と問題を1問作って各IDを返す
fn seed(conn: &rusqlite::Connection) -> (i64, i64, i64, i64) {
    let now = db::now_str();
    conn.execute("INSERT INTO subjects (name, sort_order) VALUES ('数学', 1)", [])
        .unwrap();
    let subject_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fields (subject_id, name, sort_order) VALUES (?1, '数I', 1)",
        params![subject_id],
    )
    .unwrap();
    let field_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO units (field_id, name, sort_order) VALUES (?1, '二次関数', 1)",
        params![field_id],
    )
    .unwrap();
    let unit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO problems (unit_id, title, statement_latex, answer_latex, explanation_latex, difficulty, created_at, updated_at)
         VALUES (?1, '頂点を求める', '二次関数 $y = x^2 - 4x + 7$ の頂点を求めよ。', '$(2,\\ 3)$', '平方完成する。', '基礎', ?2, ?2)",
        params![unit_id, now],
    )
    .unwrap();
    let problem_id = conn.last_insert_rowid();
    (subject_id, field_id, unit_id, problem_id)
}

fn create_project(conn: &rusqlite::Connection, name: &str) -> i64 {
    let now = db::now_str();
    conn.execute(
        "INSERT INTO projects (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![name, now],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO project_settings (project_id, booklet_title) VALUES (?1, ?2)",
        params![id, name],
    )
    .unwrap();
    id
}

/// add_problem_to_project コマンドと同じスナップショットINSERT
fn snapshot_into_project(conn: &rusqlite::Connection, project_id: i64, problem_id: i64) {
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, problem_id, snap_title, snap_statement, snap_answer, snap_explanation, snap_difficulty, snap_attachments, created_at)
         SELECT ?1, 'problem', COALESCE((SELECT MAX(sort_order) FROM project_items WHERE project_id=?1),0)+1, id, title, statement_latex, answer_latex, explanation_latex, difficulty, '[]', ?2
         FROM problems WHERE id=?3",
        params![project_id, db::now_str(), problem_id],
    )
    .unwrap();
}

#[test]
fn schema_and_hierarchy() {
    let (_dir, conn) = setup();
    let (subject_id, _field_id, unit_id, problem_id) = seed(&conn);

    // 単元削除で問題もCASCADE削除される（外部キー整合性）
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM problems WHERE id=?1", params![problem_id], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    conn.execute("DELETE FROM subjects WHERE id=?1", params![subject_id]).unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM problems", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "科目削除で配下の問題もCASCADE削除されるべき");
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM units", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    let _ = unit_id;
}

#[test]
fn search_sql_matches_title_and_statement() {
    let (_dir, conn) = setup();
    seed(&conn);
    // タイトル一致
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM problems p JOIN units u ON u.id=p.unit_id WHERE p.title LIKE '%頂点%' OR p.statement_latex LIKE '%頂点%' OR u.name LIKE '%頂点%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    // 単元名一致
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM problems p JOIN units u ON u.id=p.unit_id WHERE u.name LIKE '%二次関数%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn snapshot_is_immutable_after_bank_update() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "夏期講習第1回");
    snapshot_into_project(&conn, project_id, problem_id);

    // 問題バンク側を変更
    conn.execute(
        "UPDATE problems SET statement_latex='変更後の問題文', title='変更後タイトル' WHERE id=?1",
        params![problem_id],
    )
    .unwrap();

    let items = items_of(&conn, project_id).unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    // スナップショットは登録時のまま
    assert_eq!(item.snap_title, "頂点を求める");
    assert!(item.snap_statement.contains("x^2 - 4x + 7"));
    // バンク側更新が検知される
    assert!(item.bank_updated, "バンク更新の差分検知ができていない");
    assert!(item.source_exists);

    // 元問題を削除してもスナップショットは残る (ON DELETE SET NULL)
    conn.execute("DELETE FROM problems WHERE id=?1", params![problem_id]).unwrap();
    let items = items_of(&conn, project_id).unwrap();
    assert_eq!(items.len(), 1);
    assert!(!items[0].source_exists);
    assert_eq!(items[0].snap_title, "頂点を求める");
}

fn default_settings() -> ProjectSettings {
    ProjectSettings {
        booklet_title: "二次関数 夏期講習".into(),
        subtitle: "第1回".into(),
        target: "高1".into(),
        date_str: "2026年7月".into(),
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
        reset_numbering_per_chapter: false,
        difficulty_display: "number_side".into(),
        required_display: "required_only".into(),
    }
}

#[test]
fn tex_generation_problems_and_answers() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "テスト教材");
    snapshot_into_project(&conn, project_id, problem_id);
    // 見出し・改ページも追加
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, content, created_at) VALUES (?1,'heading',2,'第2節',?2)",
        params![project_id, db::now_str()],
    )
    .unwrap();
    snapshot_into_project(&conn, project_id, problem_id); // 同じ問題を2回追加

    let items = items_of(&conn, project_id).unwrap();
    assert_eq!(items.len(), 3);
    let settings = default_settings();

    let tex_p = build_tex("テスト教材", &settings, &items, "problems");
    assert!(tex_p.contains("\\documentclass[uplatex,a4paper,11pt]{ujarticle}"));
    assert!(tex_p.contains("\\textbf{問題1}"), "問題番号「問題1」が出力されていない");
    assert!(tex_p.contains("\\textbf{問題2}"), "見出しを挟んでも連番が継続すべき");
    assert!(tex_p.contains("x^2 - 4x + 7"));
    assert!(tex_p.contains("\\section*{第2節}"));
    assert!(tex_p.contains("氏名"));
    assert!(tex_p.contains("二次関数 夏期講習"), "{{{{TITLE}}}} が置換されていない");
    assert!(!tex_p.contains("{{BODY}}"), "プレースホルダが残っている");
    assert!(!tex_p.contains("【解答】"), "問題冊子に解答が含まれてはいけない");
    assert!(!tex_p.contains("\\begin{tcolorbox}"));

    let tex_a = build_tex("テスト教材", &settings, &items, "answers");
    assert!(tex_a.contains("【解答】"));
    assert!(tex_a.contains("【解説】"));
    assert!(tex_a.contains("平方完成"));
    assert!(!tex_a.contains("\\fbox{"));
    assert!(!tex_a.contains("\\begin{tcolorbox}"));
    assert!(tex_a.contains("\\textbf{問題1}\\par"));
    assert!(!tex_a.contains("{{ANSWER_BODY}}"));

    let mut boxed_settings = settings.clone();
    boxed_settings.box_statement_in_answers = true;
    let tex_boxed = build_tex("テスト教材", &boxed_settings, &items, "answers");
    assert!(tex_boxed.contains("\\begin{tcolorbox}[enhanced"));
    assert!(tex_boxed.contains("attach boxed title to top left={xshift=4mm,yshift*=-\\tcboxedtitleheight/2}"));
    assert!(tex_boxed.contains("boxed title style={colback=white,colframe=white"));
    assert!(tex_boxed.contains("title={\\textbf{問題1}}"));

    // 解説を含めない設定
    let mut s2 = default_settings();
    s2.include_explanation = false;
    let tex_a2 = build_tex("テスト教材", &s2, &items, "answers");
    assert!(!tex_a2.contains("【解説】"));

    // 解答2段組（全体）: 縦線付きで解答本文全体が1つのmulticolsに入る
    let mut s3 = default_settings();
    s3.two_column_mode = "all".into();
    let tex_a3 = build_tex("テスト教材", &s3, &items, "answers");
    assert!(tex_a3.contains("\\begin{multicols}{2}"));
    assert!(tex_a3.contains("\\setlength{\\columnseprule}{0.4pt}"), "2段組に縦線が入っていない");
    assert_eq!(tex_a3.matches("\\begin{multicols}{2}").count(), 1);
    // 問題文もmulticols内に含まれる
    let mc_start = tex_a3.find("\\begin{multicols}{2}").unwrap();
    assert!(tex_a3[mc_start..].contains("x^2 - 4x + 7"));

    // 解答部分のみ2段組: 問題ごとにmulticolsができ、問題文は含まれない
    let mut s5 = default_settings();
    s5.two_column_mode = "answer_only".into();
    let tex_a5 = build_tex("テスト教材", &s5, &items, "answers");
    assert_eq!(
        tex_a5.matches("\\begin{multicols}{2}").count(),
        2,
        "問題2問それぞれの解答がmulticolsで囲まれるべき"
    );
    let first_mc = tex_a5.find("\\begin{multicols}{2}").unwrap();
    assert!(
        tex_a5[..first_mc].contains("x^2 - 4x + 7"),
        "問題文は2段組の外（1段）にあるべき"
    );

    // 番号形式のカスタマイズ
    let mut s4 = default_settings();
    s4.number_format = "第{n}問".into();
    let tex_p4 = build_tex("テスト教材", &s4, &items, "problems");
    assert!(tex_p4.contains("\\textbf{第1問}"));

    // タイトル非表示（ヘッダー左のフォールバックにもタイトルが使われるため、ヘッダーも消して確認）
    let mut s6 = default_settings();
    s6.show_title = false;
    s6.show_header = false;
    let tex_p6 = build_tex("テスト教材", &s6, &items, "problems");
    assert!(!tex_p6.contains("二次関数 夏期講習"), "タイトルが出力されている");
    assert!(!tex_p6.contains("{{TITLE}}"));
    // タイトルのみ非表示の場合、タイトルブロックは空になる（ヘッダーには残ってよい）
    let mut s6b = default_settings();
    s6b.show_title = false;
    let tex_p6b = build_tex("テスト教材", &s6b, &items, "problems");
    assert!(tex_p6b.contains("{\\LARGE \\bfseries  \\par}"), "タイトルブロックが空になっていない");
    let tex_a6b = build_tex("テスト教材", &s6b, &items, "answers");
    assert!(!tex_a6b.contains("二次関数 夏期講習　解答"), "解答冊子タイトルが残っている");
    assert!(!tex_a6b.contains("　解答 \\par}"), "解答冊子の「解答」だけが残っている");
    assert!(!tex_a6b.contains("{{ANSWER_TITLE}}"));

    // ヘッダー非表示（fancyhdrの罫線も消える）
    let mut s7 = default_settings();
    s7.header_left = "講座名".into();
    s7.show_header = false;
    let tex_p7 = build_tex("テスト教材", &s7, &items, "problems");
    assert!(!tex_p7.contains("講座名"));
    assert!(tex_p7.contains("\\renewcommand{\\headrulewidth}{0pt}"), "ヘッダー罫線が消えていない");
    // 表示時はヘッダーが入る
    let mut s8 = default_settings();
    s8.header_left = "講座名".into();
    let tex_p8 = build_tex("テスト教材", &s8, &items, "problems");
    assert!(tex_p8.contains("講座名"));
}

#[test]
fn difficulty_badges_are_inline_after_problem_number() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "難易度表示テスト");
    snapshot_into_project(&conn, project_id, problem_id);

    let mut items = items_of(&conn, project_id).unwrap();
    items[0].snap_difficulty_rank = Some("A".into());
    items[0].snap_is_required = true;
    let settings = default_settings();

    let tex_p = build_tex("難易度表示テスト", &settings, &items, "problems");
    assert!(tex_p.contains("\\textbf{問題1}\\nobreak\\hspace{0.15em}{\\scriptsize ★A}\\par"));
    assert!(!tex_p.contains("\\llap{"));

    let tex_a = build_tex("難易度表示テスト", &settings, &items, "answers");
    assert!(tex_a.contains("\\textbf{問題1}\\nobreak\\hspace{0.15em}{\\scriptsize ★A}\\par"));
    assert!(!tex_a.contains("\\fbox{"));
    assert!(!tex_a.contains("\\llap{"));
}

#[test]
fn template_markers_and_custom_placeholders() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "マーカーテスト");
    snapshot_into_project(&conn, project_id, problem_id);
    let items = items_of(&conn, project_id).unwrap();
    let settings = default_settings();
    let bodies = render_bodies(&items, &settings);

    // APP_BODYマーカー方式のテンプレート
    let tpl = "\\documentclass{ujarticle}\n\\begin{document}\n% APP_BODY_START\n古い本文\n% APP_BODY_END\n\\end{document}\n";
    let doc = render_document(tpl, "problems", "マーカーテスト", &settings, &bodies);
    assert!(doc.contains("x^2 - 4x + 7"), "マーカー間に本文が挿入されていない");
    assert!(!doc.contains("古い本文"), "マーカー間の旧内容が置換されていない");

    // {{BODY}}のみのテンプレートを解答冊子に使った場合は解答が入る
    let tpl2 = "\\begin{document}\n{{BODY}}\n\\end{document}";
    let doc2 = render_document(tpl2, "answers", "t", &settings, &bodies);
    assert!(doc2.contains("【解答】"));
    assert!(!doc2.contains("\\usepackage{tcolorbox}"));
    assert!(!doc2.contains("\\fbox{"));

    let mut no_title_settings = settings.clone();
    no_title_settings.show_title = false;
    let old_answer_tpl = DEFAULT_ANSWER_TEMPLATE.replace("{{ANSWER_TITLE}}", "{{TITLE}}　解答");
    let old_doc = render_document(&old_answer_tpl, "answers", "t", &no_title_settings, &bodies);
    assert!(!old_doc.contains("　解答 \\par}"), "古い解答テンプレートで「解答」だけが残っている");

    let mut boxed_settings = settings.clone();
    boxed_settings.box_statement_in_answers = true;
    let boxed_bodies = render_bodies(&items, &boxed_settings);
    let boxed_doc = render_document(tpl2, "answers", "t", &boxed_settings, &boxed_bodies);
    assert!(boxed_doc.contains("\\usepackage{tcolorbox}"));
    assert!(boxed_doc.contains("\\tcbuselibrary{skins}"));
    assert!(boxed_doc.contains("\\begin{tcolorbox}[enhanced"));

    // {{EXPLANATION_BODY}}を持つテンプレートでは解説が分離される
    let tpl3 = "\\begin{document}\n{{ANSWER_BODY}}\n===\n{{EXPLANATION_BODY}}\n\\end{document}";
    let doc3 = render_document(tpl3, "answers", "t", &settings, &bodies);
    let sep = doc3.find("===").unwrap();
    assert!(doc3[..sep].contains("【解答】"));
    assert!(!doc3[..sep].contains("【解説】"), "解説がANSWER_BODY側に含まれている");
    assert!(doc3[sep..].contains("【解説】"));
}

#[test]
fn template_validation_and_seed() {
    let (_dir, conn) = setup();
    seed_default_template(&conn).unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM templates", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    // 既定テンプレートは警告なし
    let w = validate_templates("", DEFAULT_PROBLEM_TEMPLATE, DEFAULT_ANSWER_TEMPLATE);
    assert!(w.is_empty(), "既定テンプレートに警告: {:?}", w);
    // physics / float パッケージが含まれる
    for tpl in [DEFAULT_PROBLEM_TEMPLATE, DEFAULT_ANSWER_TEMPLATE] {
        assert!(tpl.contains("\\usepackage{physics}"));
        assert!(tpl.contains("\\usepackage{float}"));
    }
    // BODYなしテンプレートは警告あり
    let w2 = validate_templates("", "\\begin{document}\\end{document}", DEFAULT_ANSWER_TEMPLATE);
    assert!(!w2.is_empty());
    // 不明プレースホルダ警告
    let w3 = validate_templates("", DEFAULT_PROBLEM_TEMPLATE, &format!("{}\n{{{{UNKNOWN_PH}}}}", DEFAULT_ANSWER_TEMPLATE));
    assert!(w3.iter().any(|w| w.contains("UNKNOWN_PH")));
}

/// プロジェクトのテンプレートスナップショットが、テンプレート本体の変更に影響されないこと
#[test]
fn template_snapshot_is_immutable() {
    let (_dir, conn) = setup();
    seed_default_template(&conn).unwrap();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "スナップショット教材");
    // create_projectヘルパーはテンプレートを紐付けないので手動でスナップショット
    conn.execute(
        "UPDATE projects SET template_id=1,
            snap_tpl_name=(SELECT name FROM templates WHERE id=1),
            snap_tpl_problem=(SELECT problem_template FROM templates WHERE id=1),
            snap_tpl_answer=(SELECT answer_template FROM templates WHERE id=1)
         WHERE id=?1",
        params![project_id],
    )
    .unwrap();
    snapshot_into_project(&conn, project_id, problem_id);

    // テンプレート本体を変更
    conn.execute(
        "UPDATE templates SET problem_template='\\documentclass{ujarticle}\\begin{document}変更後 {{BODY}}\\end{document}' WHERE id=1",
        [],
    )
    .unwrap();

    // スナップショットは旧内容のまま
    let snap: String = conn
        .query_row("SELECT snap_tpl_problem FROM projects WHERE id=?1", params![project_id], |r| r.get(0))
        .unwrap();
    assert!(!snap.contains("変更後"));
    assert!(snap.contains("ujarticle"));
}

/// TeX Live がインストールされている場合のみ: 実際に uplatex + dvipdfmx でPDFを生成
#[test]
fn compile_pdf_with_real_tex() {
    let uplatex = which("uplatex");
    let dvipdfmx = which("dvipdfmx");
    let (Some(uplatex), Some(dvipdfmx)) = (uplatex, dvipdfmx) else {
        eprintln!("TeX環境が見つからないためPDF生成テストをスキップ");
        return;
    };

    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "PDFテスト");
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, content, heading_level, created_at) VALUES (?1,'heading',0,'第1章',1,?2)",
        params![project_id, db::now_str()],
    )
    .unwrap();
    snapshot_into_project(&conn, project_id, problem_id);
    let items = items_of(&conn, project_id).unwrap();
    // 合本 + 目次 + 番号付き見出しで実コンパイル（uplatex2回実行の検証を兼ねる）
    let mut s = default_settings();
    s.show_toc = true;
    s.number_headings = true;
    let tex = build_tex("PDFテスト", &s, &items, "combined");

    let build = tempdir::TempDir::new("kyozai-pdf").unwrap();
    let (success, pdf, log, message) =
        run_compile_with(&uplatex, &dvipdfmx, build.path(), &tex).unwrap();
    assert!(success, "{}\n{}", message, log);
    let pdf = pdf.expect("PDFパスが返ること");
    assert!(pdf.exists(), "PDFが生成されていない");
    assert!(std::fs::metadata(&pdf).unwrap().len() > 1000);

    // 軌跡問題の回帰スナップショットが、指定したgathered・左波括弧を含めて実際に組版できること。
    let trajectory_body = include_str!("fixtures/trajectory_hyperbola_midpoint.tex");
    let trajectory_tex = format!(
        "\\documentclass[uplatex]{{ujarticle}}\n\\usepackage{{amsmath,amssymb}}\n\\begin{{document}}\n{}\n\\end{{document}}\n",
        trajectory_body
    );
    let trajectory_build = tempdir::TempDir::new("kyozai-trajectory-pdf").unwrap();
    let (trajectory_success, trajectory_pdf, trajectory_log, trajectory_message) =
        run_compile_with(
            &uplatex,
            &dvipdfmx,
            trajectory_build.path(),
            &trajectory_tex,
        )
        .unwrap();
    assert!(
        trajectory_success,
        "{}\n{}",
        trajectory_message, trajectory_log
    );
    assert!(trajectory_pdf.unwrap().exists());

    // 動く線分の通過領域から回転体の体積へ進む複合問題も実際に組版できること。
    let compound_body = include_str!("fixtures/moving_segment_rotation_volume.tex");
    let compound_tex = format!(
        "\\documentclass[uplatex]{{ujarticle}}\n\\usepackage{{amsmath,amssymb}}\n\\begin{{document}}\n{}\n\\end{{document}}\n",
        compound_body
    );
    let compound_build = tempdir::TempDir::new("kyozai-compound-trajectory-pdf").unwrap();
    let (compound_success, compound_pdf, compound_log, compound_message) = run_compile_with(
        &uplatex,
        &dvipdfmx,
        compound_build.path(),
        &compound_tex,
    )
    .unwrap();
    assert!(compound_success, "{}\n{}", compound_message, compound_log);
    assert!(compound_pdf.unwrap().exists());

    // PDF添付を含む教材でもshell-escapeを使わず、事前生成した.xbbでコンパイルできること。
    let graphic_build = tempdir::TempDir::new("kyozai-pdf-graphic").unwrap();
    std::fs::copy(&pdf, graphic_build.path().join("figure.pdf")).unwrap();
    // 同名PNGが並んでいても、LaTeXが参照するPDFの寸法でfigure.xbbを作る。
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png"),
        graphic_build.path().join("figure.png"),
    )
    .unwrap();
    std::fs::copy(&pdf, graphic_build.path().join("figure-two.pdf")).unwrap();
    let graphic_tex = r#"\documentclass[uplatex]{ujarticle}
\usepackage[dvipdfmx]{graphicx}
\begin{document}
\includegraphics[width=3cm]{figure.pdf}
\includegraphics[width=3cm]{figure-two.pdf}
\end{document}"#;
    let (graphic_success, graphic_pdf, graphic_log, graphic_message) =
        run_compile_with(&uplatex, &dvipdfmx, graphic_build.path(), graphic_tex).unwrap();
    assert!(graphic_success, "{}\n{}", graphic_message, graphic_log);
    assert!(graphic_pdf.unwrap().exists());
    assert!(
        graphic_build.path().join("figure.xbb").exists(),
        "extractbbで.xbbが生成されていない"
    );
    let figure_xbb = std::fs::read_to_string(graphic_build.path().join("figure.xbb")).unwrap();
    assert!(
        figure_xbb.replace('\\', "/").contains("Title: figure.pdf"),
        "同名PNGではなく参照中PDFの寸法を使う必要があります: {figure_xbb}"
    );
    assert!(
        graphic_build.path().join("figure-two.xbb").exists(),
        "2件目の画像にも.xbbが生成されていない"
    );

    // shell escapeを明示的に無効化していることを実動作で確認する。
    let shell_build = tempdir::TempDir::new("kyozai-no-shell-escape").unwrap();
    let marker = shell_build.path().join("shell-escape-marker.txt");
    let malicious = r#"\documentclass{article}
\begin{document}
\immediate\write18{cmd /C echo unsafe>shell-escape-marker.txt}
safe
\end{document}"#;
    let _ = run_compile_with(&uplatex, &dvipdfmx, shell_build.path(), malicious);
    assert!(
        !marker.exists(),
        "-no-shell-escapeでも外部コマンドが実行されている"
    );
}

/// 章・目次・解答冊子設定・合本の出力テスト
#[test]
fn toc_headings_and_combined() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "章目次テスト");
    // 章見出し → 問題 → 節見出し
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, content, heading_level, created_at) VALUES (?1,'heading',1,'第1章 二次関数',1,?2)",
        params![project_id, db::now_str()],
    )
    .unwrap();
    snapshot_into_project(&conn, project_id, problem_id);
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, content, heading_level, created_at) VALUES (?1,'heading',3,'発展問題',2,?2)",
        params![project_id, db::now_str()],
    )
    .unwrap();
    let items = items_of(&conn, project_id).unwrap();

    // 目次あり・番号なし見出し
    let mut s = default_settings();
    s.show_toc = true;
    let tex = build_tex("章目次テスト", &s, &items, "problems");
    assert!(tex.contains("\\tableofcontents"), "目次が入っていない");
    assert!(tex.contains("\\section*{第1章 二次関数}"));
    assert!(tex.contains("\\addcontentsline{toc}{section}{第1章 二次関数}"), "番号なし見出しが目次に載らない");
    assert!(tex.contains("\\subsection*{発展問題}"), "節レベルの見出しになっていない");

    // 番号付き見出し
    let mut s2 = default_settings();
    s2.number_headings = true;
    let tex2 = build_tex("章目次テスト", &s2, &items, "problems");
    assert!(tex2.contains("\\section{第1章 二次関数}"));
    assert!(tex2.contains("\\subsection{発展問題}"));

    // 解答冊子に問題文を含めない
    let mut s3 = default_settings();
    s3.include_statement_in_answers = false;
    let tex3 = build_tex("章目次テスト", &s3, &items, "answers");
    assert!(!tex3.contains("x^2 - 4x + 7"), "問題文が解答冊子に含まれている");
    assert!(tex3.contains("【解答】"));
    assert!(!tex3.contains("\\fbox{"));

    // 合本: 問題本文 → 改ページ → 解答見出し → 解答本文
    let tex4 = build_tex("章目次テスト", &default_settings(), &items, "combined");
    let clear = tex4.find("\\clearpage").expect("合本に改ページがない");
    assert!(tex4[..clear].contains("x^2 - 4x + 7"), "合本の前半に問題がない");
    assert!(tex4[clear..].contains("\\textbf{解答}"), "合本に解答見出しがない");
    assert!(tex4[clear..].contains("【解答】"), "合本の後半に解答がない");
    assert!(!tex4.contains("{{BODY}}"));

    // 合本 + 目次 + 2段組
    let mut s5 = default_settings();
    s5.show_toc = true;
    s5.two_column_mode = "all".into();
    let tex5 = build_tex("章目次テスト", &s5, &items, "combined");
    assert!(tex5.contains("\\tableofcontents"));
    assert!(tex5.contains("\\begin{multicols}{2}"));
}

/// 章ごとの問題番号リセット・章番号付き「2-1」形式・目次の問題/解答区別
#[test]
fn chapter_numbering_and_toc_distinction() {
    let (_dir, conn) = setup();
    let (_s, _f, _u, problem_id) = seed(&conn);
    let project_id = create_project(&conn, "章番号テスト");
    // 第1章 → 問題×2 → 第2章 → 問題×1 → 番号なし章 → 問題×1
    let add_heading = |name: &str, numbered: i64| {
        let order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order),0)+1 FROM project_items WHERE project_id=?1",
                params![project_id],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO project_items (project_id, item_type, sort_order, content, heading_level, heading_numbered, created_at) VALUES (?1,'heading',?2,?3,1,?4,?5)",
            params![project_id, order, name, numbered, db::now_str()],
        )
        .unwrap();
    };
    add_heading("第1章", 1);
    snapshot_into_project(&conn, project_id, problem_id);
    snapshot_into_project(&conn, project_id, problem_id);
    add_heading("第2章", 1);
    snapshot_into_project(&conn, project_id, problem_id);
    add_heading("補充問題", 0); // この章は番号なし
    snapshot_into_project(&conn, project_id, problem_id);
    let items = items_of(&conn, project_id).unwrap();

    // 章ごとリセット + 番号付き章 → 「問題1-1」「問題1-2」「問題2-1」、番号なし章は「問題1」
    let mut s = default_settings();
    s.number_headings = true;
    s.reset_numbering_per_chapter = true;
    s.show_toc = true;
    let tex = build_tex("章番号テスト", &s, &items, "problems");
    assert!(tex.contains("\\textbf{問題1-1}"), "1-1形式になっていない:\n{}", tex);
    assert!(tex.contains("\\textbf{問題1-2}"));
    assert!(tex.contains("\\textbf{問題2-1}"), "第2章で2-1にならない");
    assert!(tex.contains("\\section{第1章}"));
    assert!(tex.contains("\\section*{補充問題}"), "番号なし指定の章に番号が付いている");
    // 番号なし章では通し番号なしのリセット番号
    assert!(tex.contains("\\textbf{問題1}\\par"), "番号なし章で問題1にリセットされない");

    // 解答冊子: 見出しは番号なしコマンド（\section*）だが、章番号がある場合は
    // 見出し・目次とも「1　第1章」形式（（解答）サフィックスは付けない）
    let tex_a = build_tex("章番号テスト", &s, &items, "answers");
    assert!(tex_a.contains("\\addcontentsline{toc}{section}{1　第1章}"), "解答側の目次に章番号が付かない");
    assert!(!tex_a.contains("（解答）"), "目次に不要な（解答）が付いている");
    assert!(!tex_a.contains("\\section{第1章}"), "解答側の見出しが番号付きコマンドになっている");
    assert!(tex_a.contains("\\section*{1　第1章}"), "解答側見出しに章番号表記がない");
    // 番号なし章はそのままの見出しで目次に載る
    assert!(tex_a.contains("\\addcontentsline{toc}{section}{補充問題}"));
    assert!(tex_a.contains("\\textbf{問題1-1}"), "解答側の問題番号が一致しない");

    // 合本: 解答編の区切りが目次に載る
    let tex_c = build_tex("章番号テスト", &s, &items, "combined");
    assert!(tex_c.contains("\\addcontentsline{toc}{section}{──── 解答編 ────}"));

    // リセットなし → 通し番号のまま（プレフィックスなし）
    let mut s2 = default_settings();
    s2.number_headings = true;
    s2.reset_numbering_per_chapter = false;
    let tex2 = build_tex("章番号テスト", &s2, &items, "problems");
    assert!(tex2.contains("\\textbf{問題1}\\par"));
    assert!(tex2.contains("\\textbf{問題3}\\par"), "リセットなしで通し番号にならない");
    assert!(!tex2.contains("問題1-1"));
}

/// 単問プレビューがテンプレートのプリアンブルを引き継ぐこと
#[test]
fn preview_doc_uses_template_preamble() {
    use kyozai_kobo_lib::commands::latex::build_preview_doc;
    let tpl = "\\documentclass[uplatex]{ujarticle}\n\\usepackage{physics}\n\\usepackage{mypkg}\n\\lhead{{{HEADER_LEFT}}}\n\\newcommand{\\mycmd}{X}\n\\begin{document}\n{{BODY}}\n\\end{document}\n";
    let doc = build_preview_doc(tpl, "問題文 $\\dv{y}{x}$", "解答です", "");
    assert!(doc.contains("\\usepackage{mypkg}"), "テンプレートのパッケージが引き継がれない");
    assert!(doc.contains("\\newcommand{\\mycmd}"), "独自コマンドが引き継がれない");
    assert!(!doc.contains("{{HEADER_LEFT}}"), "プレースホルダが残っている");
    assert!(doc.contains("\\pagestyle{empty}"));
    assert!(doc.contains("問題文"));
    assert!(doc.contains("【解答】"));
    assert!(!doc.contains("【解説】"), "空の解説が出力されている");
    // \begin{document} の無いテンプレートでは既定のプリアンブルにフォールバック
    let doc2 = build_preview_doc("junk", "S", "", "");
    assert!(doc2.contains("\\documentclass[uplatex,a4paper,11pt]{ujarticle}"));
    assert!(doc2.contains("\\usepackage{physics}"));
}

/// 問題バンクのエクスポート → 別DBへインポートのラウンドトリップ
#[test]
fn bank_export_import_roundtrip() {
    use kyozai_kobo_lib::commands::bank::{apply_bank_import, build_bank_export};

    let (dir_a, conn_a) = setup();
    let (_s, _f, unit_id, problem_id) = seed(&conn_a);
    // タグと添付を付与
    conn_a.execute("INSERT INTO tags (name) VALUES ('平方完成')", []).unwrap();
    conn_a
        .execute("INSERT INTO problem_tags (problem_id, tag_id) VALUES (?1, 1)", params![problem_id])
        .unwrap();
    let att_dir_a = dir_a.path().join("attachments");
    std::fs::create_dir_all(&att_dir_a).unwrap();
    std::fs::write(att_dir_a.join("imgabc.png"), b"fake-png-data").unwrap();
    conn_a
        .execute(
            "INSERT INTO attachments (problem_id, file_name, stored_name, created_at) VALUES (?1, 'graph.png', 'imgabc.png', '2026-01-01')",
            params![problem_id],
        )
        .unwrap();
    conn_a
        .execute(
            "UPDATE problems SET statement_latex = statement_latex || ' \\includegraphics{imgabc.png}' WHERE id=?1",
            params![problem_id],
        )
        .unwrap();

    // 単元単位でエクスポート
    let data = build_bank_export(&conn_a, &att_dir_a, "unit", Some(unit_id), None).unwrap();
    assert_eq!(data.subjects.len(), 1);
    assert_eq!(data.subjects[0].fields[0].units[0].problems.len(), 1);
    assert!(!data.subjects[0].fields[0].units[0].problems[0].attachments[0].data_base64.is_empty());

    // 新しいDBへインポート
    let (dir_b, conn_b) = setup();
    let att_dir_b = dir_b.path().join("attachments");
    let result = apply_bank_import(&conn_b, &att_dir_b, &data).unwrap();
    assert_eq!(result.subjects_created, 1);
    assert_eq!(result.units_created, 1);
    assert_eq!(result.problems_imported, 1);

    // 問題・タグ・添付が復元され、LaTeX中のファイル名が新しい保存名に置換されている
    let (title, statement): (String, String) = conn_b
        .query_row("SELECT title, statement_latex FROM problems LIMIT 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(title, "頂点を求める");
    assert!(!statement.contains("imgabc.png"), "旧ファイル名が残っている");
    let new_stored: String = conn_b
        .query_row("SELECT stored_name FROM attachments LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(statement.contains(&new_stored), "新ファイル名に置換されていない");
    assert!(att_dir_b.join(&new_stored).exists(), "添付ファイルが復元されていない");
    let tag: String = conn_b
        .query_row(
            "SELECT t.name FROM tags t JOIN problem_tags pt ON pt.tag_id=t.id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tag, "平方完成");

    // 同じデータを再インポートすると階層はマージされ、問題だけ増える
    let result2 = apply_bank_import(&conn_b, &att_dir_b, &data).unwrap();
    assert_eq!(result2.subjects_created, 0, "同名科目が重複作成された");
    assert_eq!(result2.units_created, 0);
    assert_eq!(result2.problems_imported, 1);
    let n: i64 = conn_b.query_row("SELECT COUNT(*) FROM problems", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
}

/// インポート途中の失敗（添付の書き込みエラー）でトランザクションを巻き戻すと、
/// 部分的な科目・分野・単元・問題が一切残らないこと（import_bank と同じ手順）
#[test]
fn bank_import_rolls_back_on_failure() {
    use kyozai_kobo_lib::commands::bank::{apply_bank_import, build_bank_export};

    let (dir_a, conn_a) = setup();
    let (_s, _f, unit_id, problem_id) = seed(&conn_a);
    let att_dir_a = dir_a.path().join("attachments");
    std::fs::create_dir_all(&att_dir_a).unwrap();
    std::fs::write(att_dir_a.join("imgroll.png"), b"fake-png").unwrap();
    conn_a
        .execute(
            "INSERT INTO attachments (problem_id, file_name, stored_name, created_at) VALUES (?1, 'g.png', 'imgroll.png', '2026-01-01')",
            params![problem_id],
        )
        .unwrap();
    let data = build_bank_export(&conn_a, &att_dir_a, "unit", Some(unit_id), None).unwrap();

    // 添付保存先をディレクトリではなく既存ファイルにして書き込みを失敗させる
    let (dir_b, mut conn_b) = setup();
    let broken_att_dir = dir_b.path().join("attachments-as-file");
    std::fs::write(&broken_att_dir, b"not a directory").unwrap();
    let tx = conn_b.transaction().unwrap();
    let result = apply_bank_import(&tx, &broken_att_dir, &data);
    assert!(result.is_err(), "添付書き込み失敗でもエラーにならなかった");
    drop(tx); // rollback

    for table in ["subjects", "fields", "units", "problems", "attachments"] {
        let n: i64 = conn_b
            .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{} に部分インポートが残っている", table);
    }
}

/// APP_BODYマーカーと {{BODY}} を両方持つテンプレートは本文が二重挿入されるため警告する
#[test]
fn template_validation_warns_on_double_body_insertion() {
    let both = "\\begin{document}\n% APP_BODY_START\n% APP_BODY_END\n{{BODY}}\n\\end{document}";
    let warnings = validate_templates("", both, "");
    assert!(
        warnings.iter().any(|w| w.contains("二重に挿入")),
        "問題冊子側の二重挿入警告が出ない: {:?}",
        warnings
    );
    let answer_both =
        "\\begin{document}\n% APP_BODY_START\n% APP_BODY_END\n{{ANSWER_BODY}}\n\\end{document}";
    let warnings = validate_templates("", DEFAULT_PROBLEM_TEMPLATE, answer_both);
    assert!(
        warnings.iter().any(|w| w.contains("二重に挿入")),
        "解答冊子側の二重挿入警告が出ない: {:?}",
        warnings
    );
    // 既定テンプレート（{{BODY}}のみ）では警告しない
    let warnings = validate_templates("", DEFAULT_PROBLEM_TEMPLATE, DEFAULT_ANSWER_TEMPLATE);
    assert!(
        !warnings.iter().any(|w| w.contains("二重に挿入")),
        "正常テンプレートに誤警告: {:?}",
        warnings
    );
}

fn which(name: &str) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("where.exe").arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(|l| std::path::PathBuf::from(l.trim()))
}
