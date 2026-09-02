//! 開発検証用: GUIなしで実際のCodexへ定石抽出を投げ、一般化の粒度を目視確認する。
//! 使い捨てのデータフォルダを使うので、実アプリのDBとユーザーデータには触れない。
//!
//! 環境変数:
//!   KK_CASE        … 実行するケース（a / b / c / d、カンマ区切りで複数可。既定は a）
//!   KK_STYLE       … 抽出方針（standard / more_general / exam_pattern_focused / custom）
//!   KK_INSTRUCTION … 抽出への追加指示
//!   KK_DATA_DIR … データフォルダ（省略時は %TEMP%\kyozai-pattern-check）
//! 実行: cargo run --example pattern_extraction_check

use kyozai_kobo_lib::state::AppState;
use kyozai_kobo_lib::{ai, commands, db};
use rusqlite::params;
use std::path::PathBuf;
use std::sync::Arc;

struct Case {
    key: &'static str,
    title: &'static str,
    statement: &'static str,
    answer: &'static str,
    expected: &'static str,
    /// 抽出前に定石ライブラリへ入れておく上位定石（既存検索・特殊化推奨の検証用）。
    seed_general: Option<SeedPattern>,
}

struct SeedPattern {
    title: &'static str,
    summary: &'static str,
    situation: &'static str,
    principle: &'static str,
    strategy: &'static str,
    tags: &'static [&'static str],
    domains: &'static [&'static str],
    goals: &'static [&'static str],
    operations: &'static [&'static str],
    structures: &'static [&'static str],
    situations: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        key: "a",
        title: "線分の通過領域",
        statement: r"座標平面上に2点 \(\mathrm{A}(a,0)\), \(\mathrm{B}(0,1-a)\) がある。\(a\) が \(0\leqq a\leqq 1\) の範囲を動くとき、線分ABが通過する領域を図示せよ。",
        answer: r"線分AB上の点Xは、\(0\leqq t\leqq 1\) を満たす \(t\) を用いて \(\mathrm{X}=(1-t)\mathrm{A}+t\mathrm{B}\) と表せる。座標で書くと \(x=(1-t)a\), \(y=t(1-a)\) である。
点 \((x,y)\) が通過領域に属することは、\(0\leqq a\leqq 1\), \(0\leqq t\leqq 1\) を満たす \(a\), \(t\) が存在することと同値である。
\(x\geqq 0\), \(y\geqq 0\) のもとで \(t\) を消去すると、\(\sqrt{x}+\sqrt{y}\leqq 1\) が得られる。
よって求める領域は \(x\geqq 0\), \(y\geqq 0\), \(\sqrt{x}+\sqrt{y}\leqq 1\) を満たす部分である。",
        expected: "「媒介変数表示を存在条件として扱う」程度の一般性。「線分通過領域の存在条件化」で止まらないこと",
        seed_general: None,
    },
    Case {
        key: "b",
        title: "回転体の体積",
        statement: r"曲線 \(y=\sqrt{x}\)、直線 \(x=1\)、\(x=4\) および \(x\) 軸で囲まれた部分を、\(x\) 軸のまわりに1回転してできる立体の体積 \(V\) を求めよ。",
        answer: r"\(x\) 軸に垂直な平面 \(x=t\)（\(1\leqq t\leqq 4\)）による断面は、半径 \(\sqrt{t}\) の円板である。
よって断面積は \(S(t)=\pi t\) である。
これを \(t\) について積分して \(V=\int_1^4 \pi t\,dt=\pi\left[\frac{t^2}{2}\right]_1^4=\frac{15}{2}\pi\) となる。",
        expected: "既存の上位定石が見つかり、新規作成ではなく特殊化・具体例・候補追加を推奨すること",
        seed_general: Some(SeedPattern {
            title: "立体の体積を求めるために断面積を積分する",
            summary: "立体の体積は、ある方向に垂直な平面で切った断面積を、その方向について積分して求める。",
            situation: "立体の体積を求めたく、切り口の面積を位置の関数として表せるとき。",
            principle: "断面積を位置の関数として表し、立体が存在する範囲でその関数を積分すれば体積が得られる。",
            strategy: "切る方向を決めて断面積を位置の関数で表す",
            tags: &["体積", "断面積", "積分"],
            domains: &["微分と積分"],
            goals: &["体積の決定"],
            operations: &["積分", "断面の把握"],
            structures: &["立体", "断面"],
            situations: &["体積を求める"],
        }),
    },
    Case {
        key: "c",
        title: "恒等式の係数決定",
        statement: r"すべての実数 \(x\) について \(a(x-1)(x-2)+b(x-2)x+c\,x(x-1)=x^2+1\) が成り立つように、定数 \(a\), \(b\), \(c\) の値を求めよ。",
        answer: r"すべての実数 \(x\) で成り立つので、特に \(x=0,1,2\) を代入してよい。
\(x=0\) のとき \(2a=1\) より \(a=\frac{1}{2}\)。
\(x=1\) のとき \(-b=2\) より \(b=-2\)。
\(x=2\) のとき \(2c=5\) より \(c=\frac{5}{2}\)。
逆にこのとき両辺は一致するので、\(a=\frac{1}{2}\), \(b=-2\), \(c=\frac{5}{2}\) である。",
        expected: "「特殊値代入によって必要条件を抽出する」程度の一般性。十分性の確認がcautionsに入ること",
        seed_general: None,
    },
    Case {
        key: "d",
        title: "二次関数の最小値",
        statement: r"\(a\) を実数の定数とする。\(0\leqq x\leqq 2\) における関数 \(f(x)=x^2-2ax+3\) の最小値を \(a\) を用いて表せ。",
        answer: r"\(f(x)=(x-a)^2+3-a^2\) と変形できるので、頂点の \(x\) 座標は \(a\) である。
\(a<0\) のとき、区間で単調増加だから最小値は \(f(0)=3\)。
\(0\leqq a\leqq 2\) のとき、頂点が区間に含まれるから最小値は \(f(a)=3-a^2\)。
\(2<a\) のとき、区間で単調減少だから最小値は \(f(2)=7-4a\)。",
        expected: "「平方完成する」ではなく、値域・最大最小を調べる目的を含む粒度",
        seed_general: None,
    },
    Case {
        key: "e",
        title: "対数の差の評価",
        statement: r"\(0<a<b\) のとき、\(\dfrac{b-a}{b}<\log\dfrac{b}{a}<\dfrac{b-a}{a}\) が成り立つことを示せ。",
        answer: r"\(\log\dfrac{b}{a}=\log b-\log a\) である。
\(f(x)=\log x\) は区間 \([a,b]\) で連続、\((a,b)\) で微分可能だから、平均値の定理より
\(\dfrac{\log b-\log a}{b-a}=\dfrac{1}{c}\), \(a<c<b\) を満たす \(c\) が存在する。
\(a<c<b\) より \(\dfrac{1}{b}<\dfrac{1}{c}<\dfrac{1}{a}\) であり、\(b-a>0\) を掛けて \(\dfrac{b-a}{b}<\log b-\log a<\dfrac{b-a}{a}\) を得る。
別解として、\(\log b-\log a=\displaystyle\int_a^b \dfrac{dx}{x}\) と表し、\(a\leqq x\leqq b\) で \(\dfrac{1}{b}\leqq\dfrac{1}{x}\leqq\dfrac{1}{a}\) を積分しても同じ不等式が得られる。",
        expected: "「関数値の差 f(b)-f(a) の扱い」相当の粒度で keep_as_is が返り、無理に薄められないこと",
        seed_general: None,
    },
    Case {
        key: "f",
        title: "面積の差の最大",
        statement: r"\(a\geqq 1\) とする。\(xy\) 平面において、不等式 \[0\leqq x\leqq rac{\pi}{2},\qquad 1\leqq y\leqq a\sin x\] によって定められる領域の面積を \(S_1\)、不等式 \[0\leqq x\leqq rac{\pi}{2},\qquad 0\leqq y\leqq a\sin x,\qquad 0\leqq y\leqq 1\] によって定められる領域の面積を \(S_2\) とする。\(S_2-S_1\) を最大とする \(a\) の値とその最大値を求めよ。",
        answer: r"\(a\sin x=1\) となる \(x\) を \(lpha\)（\(0<lpha\leqqrac{\pi}{2}\)）とおくと \(a\sinlpha=1\) である。
\(0\leqq x\leqqlpha\) では \(a\sin x\leqq 1\)、\(lpha\leqq x\leqqrac{\pi}{2}\) では \(a\sin x\geqq 1\) だから、積分区間を \(lpha\) で分けて
\[S_1=\int_lpha^{rac{\pi}{2}}(a\sin x-1)\,dx,\qquad S_2=\int_0^{lpha}a\sin x\,dx+\int_lpha^{rac{\pi}{2}}1\,dx\]
となる。これを計算して \(S_2-S_1\) を \(a,lpha\) で表し、\(a=\dfrac{1}{\sinlpha}\) を用いて \(a\) を消去すると、\(S_2-S_1\) は \(lpha\) だけの関数になる。
これを \(lpha\) で微分して増減を調べ、最大値を与える \(lpha\)、したがって \(a\) を求める。",
        expected: "「求めにくいものを文字でおいて計算を進める」という指示どおりの粒度になること",
        seed_general: None,
    },
];

fn seed_problem(state: &AppState, case: &Case) -> i64 {
    let conn = state.conn.lock().unwrap();
    conn.execute("INSERT INTO subjects(name,sort_order) VALUES ('数学',1)", [])
        .ok();
    let subject_id: i64 = conn
        .query_row("SELECT id FROM subjects WHERE name='数学'", [], |row| {
            row.get(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO fields(subject_id,name,sort_order) VALUES (?1,'検証',1)",
        params![subject_id],
    )
    .ok();
    let field_id: i64 = conn
        .query_row("SELECT id FROM fields WHERE name='検証'", [], |row| {
            row.get(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO units(field_id,name,sort_order) VALUES (?1,'定石抽出',1)",
        params![field_id],
    )
    .ok();
    let unit_id: i64 = conn
        .query_row("SELECT id FROM units WHERE name='定石抽出'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let now = db::now_str();
    conn.execute(
        "INSERT INTO problems(unit_id,title,statement_latex,answer_latex,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?5)",
        params![unit_id, case.title, case.statement, case.answer, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

/// KK_IMAGE にPNGのパスを渡すと、画像からの定石取り込みだけを試す。
fn run_image_import(state: &Arc<AppState>, path: &str) {
    let bytes = std::fs::read(path).expect("画像を読めません");
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let name = ai::store_input_image(state, &data, "card.png")
        .expect("画像を保存できません");
    let stored = name
        .get("name")
        .and_then(|v| v.as_str())
        .expect("保存名")
        .to_string();
    let job = commands::patterns::start_pattern_image_import(state, vec![stored], None)
        .expect("取り込みジョブを開始できません");
    let job_id = job.get("id").and_then(|v| v.as_i64()).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let (status, message, error, result): (String, String, String, String) = {
            let conn = state.conn.lock().unwrap();
            conn.query_row(
                "SELECT status,progress_message,error_message,structured_result_json
                 FROM ai_conversion_jobs WHERE id=?1",
                params![job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
        };
        if status == "completed" {
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            println!("{}", serde_json::to_string_pretty(&parsed).unwrap());
            return;
        }
        if status == "failed" || status == "cancelled" {
            println!("IMAGE_{}: {} / {}", status.to_uppercase(), error, message);
            return;
        }
    }
}

fn main() {
    let data_dir = std::env::var("KK_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("kyozai-pattern-check"));
    std::fs::create_dir_all(&data_dir).expect("データフォルダを作成できません");
    let conn = db::open_db(&data_dir).expect("DBを開けません");
    let state = Arc::new(AppState::new(conn, data_dir.clone()));
    ai::start_worker(state.clone());
    println!("DATA_DIR={}", data_dir.display());

    if let Ok(path) = std::env::var("KK_IMAGE") {
        run_image_import(&state, &path);
        println!("
DONE");
        return;
    }

    let selected = std::env::var("KK_CASE").unwrap_or_else(|_| "a".into());
    let keys: Vec<String> = selected
        .split(',')
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect();

    for case in CASES.iter().filter(|case| keys.iter().any(|k| k == case.key)) {
        println!("\n================ CASE {} : {} ================", case.key, case.title);
        println!("期待: {}", case.expected);
        if let Some(seed) = case.seed_general.as_ref() {
            let id =
                commands::patterns::create_pattern(&state, seed.title.into(), "strategy".into())
                    .unwrap();
            let current = commands::patterns::get_pattern(&state, id).unwrap();
            commands::patterns::update_pattern(
                &state,
                kyozai_kobo_lib::models::PatternUpdate {
                    id,
                    expected_version: Some(current.version),
                    title: seed.title.into(),
                    summary: seed.summary.into(),
                    pattern_type: "strategy".into(),
                    situation: seed.situation.into(),
                    principle: seed.principle.into(),
                    cautions: String::new(),
                    examples: String::new(),
                    source_note: "検証用に用意した上位定石".into(),
                    tags: seed.tags.iter().map(|v| v.to_string()).collect(),
                    facets: kyozai_kobo_lib::models::PatternFacets {
                        domains: seed.domains.iter().map(|v| v.to_string()).collect(),
                        goals: seed.goals.iter().map(|v| v.to_string()).collect(),
                        operations: seed.operations.iter().map(|v| v.to_string()).collect(),
                        structures: seed.structures.iter().map(|v| v.to_string()).collect(),
                        situations: seed.situations.iter().map(|v| v.to_string()).collect(),
                    },
                    strategies: vec![kyozai_kobo_lib::models::PatternStrategyInput {
                        id: None,
                        parent_strategy_id: None,
                        title: seed.strategy.into(),
                        description: "断面積を位置の関数として表す。".into(),
                        condition: "切り口の面積を位置の式で書けるとき。".into(),
                        reasoning: "体積は断面積の積分として表せる。".into(),
                        branch_label: String::new(),
                        sort_order: 1,
                    }],
                },
            )
            .unwrap();
            println!("既存の上位定石を用意しました: #{id} {}", seed.title);
        }
        let problem_id = seed_problem(&state, case);
        // KK_STYLE / KK_INSTRUCTION で抽出方針も検証できるようにする。
        let style = std::env::var("KK_STYLE").ok();
        let instruction = std::env::var("KK_INSTRUCTION").ok();
        let job = match commands::patterns::start_pattern_extraction(
            &state,
            problem_id,
            style,
            instruction,
        ) {
            Ok(job) => job,
            Err(error) => {
                println!("START_ERROR: {error}");
                continue;
            }
        };
        let job_id = job.get("id").and_then(|v| v.as_i64()).expect("ジョブID");
        let mut last = String::new();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let (status, message, error_message, result): (String, String, String, String) = {
                let conn = state.conn.lock().unwrap();
                conn.query_row(
                    "SELECT status,progress_message,error_message,structured_result_json
                     FROM ai_conversion_jobs WHERE id=?1",
                    params![job_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
            };
            if message != last {
                println!("  [{status}] {message}");
                last = message;
            }
            if status == "completed" {
                let value: serde_json::Value = serde_json::from_str(&result).unwrap();
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                // KK_EDIT に指示を入れると、保存済み定石をAIで編集する経路を試す。
                if let Ok(instruction) = std::env::var("KK_EDIT") {
                    let extraction: kyozai_kobo_lib::models::PatternExtractionResult =
                        serde_json::from_value(value.clone()).unwrap();
                    let first = extraction.patterns.into_iter().next().unwrap();
                    let saved = commands::patterns::apply_pattern_proposal(
                        &state,
                        kyozai_kobo_lib::models::ApplyPatternProposalPayload {
                            problem_id,
                            source_reference: String::new(),
                            proposal: first,
                            action: "create_new".into(),
                            target_pattern_id: None,
                            link_relation_type: None,
                            parent_pattern_id: None,
                        },
                    )
                    .unwrap();
                    let before = commands::patterns::get_pattern(&state, saved.pattern_id).unwrap();
                    println!("---- AIで編集 ----");
                    println!("指示  : {instruction}");
                    println!("before: {}", before.title);
                    for strategy in &before.strategies {
                        println!("   ● {}", strategy.title);
                    }
                    let job = commands::patterns::start_pattern_edit(
                        &state,
                        saved.pattern_id,
                        instruction,
                    )
                    .unwrap();
                    let eid = job.get("id").and_then(|v| v.as_i64()).unwrap();
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let (st, err, res): (String, String, String) = {
                            let conn = state.conn.lock().unwrap();
                            conn.query_row(
                                "SELECT status,error_message,structured_result_json
                                 FROM ai_conversion_jobs WHERE id=?1",
                                params![eid],
                                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                            )
                            .unwrap()
                        };
                        if st == "completed" {
                            let edited: kyozai_kobo_lib::models::PatternExtractionResult =
                                serde_json::from_str(&res).unwrap();
                            let proposal = edited.patterns.into_iter().next().unwrap();
                            println!("after : {}", proposal.title);
                            for strategy in &proposal.strategies {
                                println!("   ● {}", strategy.title);
                                println!("      {}", strategy.description);
                            }
                            let version = commands::patterns::apply_pattern_edit(
                                &state,
                                saved.pattern_id,
                                Some(before.version),
                                proposal,
                            )
                            .unwrap();
                            let saved_now =
                                commands::patterns::get_pattern(&state, saved.pattern_id).unwrap();
                            println!(
                                "保存後: v{} / {} / 履歴{}件",
                                version,
                                saved_now.title,
                                commands::patterns::list_pattern_versions(&state, saved.pattern_id)
                                    .unwrap()
                                    .len()
                            );
                            break;
                        }
                        if st == "failed" || st == "cancelled" {
                            println!("EDIT_{}: {}", st.to_uppercase(), err);
                            break;
                        }
                    }
                }
                // KK_GENERALIZE=1 のとき、先頭候補へ「さらに一般化」を1回かける。
                if std::env::var("KK_GENERALIZE").is_ok() {
                    let extraction: kyozai_kobo_lib::models::PatternExtractionResult =
                        serde_json::from_value(value).unwrap();
                    let first = extraction.patterns.into_iter().next().unwrap();
                    println!("---- さらに一般化 ----");
                    println!("before: {} [{}]", first.title, first.generalization_decision);
                    match commands::patterns::start_pattern_generalization(
                        &state, problem_id, first,
                    ) {
                        Ok(job) => {
                            let gid = job.get("id").and_then(|v| v.as_i64()).unwrap();
                            loop {
                                std::thread::sleep(std::time::Duration::from_secs(3));
                                let (st, err, res): (String, String, String) = {
                                    let conn = state.conn.lock().unwrap();
                                    conn.query_row(
                                        "SELECT status,error_message,structured_result_json
                                         FROM ai_conversion_jobs WHERE id=?1",
                                        params![gid],
                                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                                    )
                                    .unwrap()
                                };
                                if st == "completed" {
                                    let v: serde_json::Value =
                                        serde_json::from_str(&res).unwrap();
                                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                                    break;
                                }
                                if st == "failed" || st == "cancelled" {
                                    println!("GENERALIZE_{}: {}", st.to_uppercase(), err);
                                    break;
                                }
                            }
                        }
                        Err(error) => println!("GENERALIZE_START_ERROR: {error}"),
                    }
                }
                break;
            }
            if status == "failed" || status == "cancelled" {
                println!("  JOB_{}: {}", status.to_uppercase(), error_message);
                break;
            }
        }
    }
    println!("\nDONE");
}
