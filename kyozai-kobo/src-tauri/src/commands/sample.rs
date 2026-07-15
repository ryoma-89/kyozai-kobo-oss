use crate::db::now_str;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection};

fn insert_problem(
    conn: &Connection,
    unit_id: i64,
    title: &str,
    statement: &str,
    answer: &str,
    explanation: &str,
    difficulty: &str,
    tags: &[&str],
) -> rusqlite::Result<()> {
    let now = now_str();
    conn.execute(
        "INSERT INTO problems (unit_id, title, statement_latex, answer_latex, explanation_latex, difficulty, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![unit_id, title, statement, answer, explanation, difficulty, now],
    )?;
    let pid = conn.last_insert_rowid();
    for t in tags {
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![t])?;
        let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| r.get(0))?;
        conn.execute(
            "INSERT OR IGNORE INTO problem_tags (problem_id, tag_id) VALUES (?1, ?2)",
            params![pid, tag_id],
        )?;
    }
    Ok(())
}

fn add_node(conn: &Connection, table: &str, parent_col: Option<&str>, parent_id: Option<i64>, name: &str, order: i64) -> rusqlite::Result<i64> {
    match parent_col {
        None => {
            conn.execute(
                &format!("INSERT INTO {} (name, sort_order) VALUES (?1, ?2)", table),
                params![name, order],
            )?;
        }
        Some(pc) => {
            conn.execute(
                &format!("INSERT INTO {} ({}, name, sort_order) VALUES (?1, ?2, ?3)", table, pc),
                params![parent_id.unwrap(), name, order],
            )?;
        }
    }
    Ok(conn.last_insert_rowid())
}

pub fn create_sample_data(state: &AppState) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;

    let math = add_node(&conn, "subjects", None, None, "数学", 1).map_err(err_str)?;
    let su1 = add_node(&conn, "fields", Some("subject_id"), Some(math), "数I", 1).map_err(err_str)?;
    let sua = add_node(&conn, "fields", Some("subject_id"), Some(math), "数A", 2).map_err(err_str)?;

    let u_niji = add_node(&conn, "units", Some("field_id"), Some(su1), "二次関数", 1).map_err(err_str)?;
    let u_saidai = add_node(&conn, "units", Some("field_id"), Some(su1), "最大・最小", 2).map_err(err_str)?;
    let u_hanbetsu = add_node(&conn, "units", Some("field_id"), Some(su1), "判別式", 3).map_err(err_str)?;
    let _u_zukei = add_node(&conn, "units", Some("field_id"), Some(su1), "図形と計量", 4).map_err(err_str)?;
    let u_baai = add_node(&conn, "units", Some("field_id"), Some(sua), "場合の数", 1).map_err(err_str)?;

    insert_problem(
        &conn,
        u_niji,
        "二次関数の頂点",
        "二次関数 $y = x^2 - 4x + 7$ を $y = a(x-p)^2 + q$ の形に変形し、頂点の座標を求めよ。",
        "$y = x^2 - 4x + 7 = (x-2)^2 + 3$\n\nよって頂点の座標は $(2,\\ 3)$ である。",
        "平方完成を行う。$x^2 - 4x$ の部分に注目し、\n\\[ x^2 - 4x = (x-2)^2 - 4 \\]\nと変形することがポイントである。",
        "基礎",
        &["平方完成", "頂点"],
    )
    .map_err(err_str)?;

    insert_problem(
        &conn,
        u_niji,
        "二次関数の決定",
        "放物線 $y = ax^2 + bx + c$ が3点 $(0,\\ 1)$，$(1,\\ 2)$，$(-1,\\ 6)$ を通るとき、定数 $a,\\ b,\\ c$ の値を求めよ。",
        "$(0,\\ 1)$ を通るので $c = 1$。\n\n$(1,\\ 2)$ より $a + b + c = 2$，$(-1,\\ 6)$ より $a - b + c = 6$。\n\nこれを解いて $a = 3,\\ b = -2,\\ c = 1$。",
        "通る点の座標を代入して連立方程式を作る。$x=0$ の点から $c$ が直ちに決まることに着目すると計算が楽になる。",
        "標準",
        &["二次関数の決定", "連立方程式"],
    )
    .map_err(err_str)?;

    insert_problem(
        &conn,
        u_saidai,
        "定義域付き最大・最小",
        "二次関数 $y = x^2 - 2x + 3$（$0 \\leqq x \\leqq 3$）の最大値と最小値を求めよ。",
        "$y = (x-1)^2 + 2$ より軸は $x = 1$。\n\n\\begin{itemize}\n\\item 最小値：$x = 1$ のとき $y = 2$\n\\item 最大値：$x = 3$ のとき $y = 6$\n\\end{itemize}",
        "平方完成して軸の位置を確認する。軸 $x=1$ は定義域 $0 \\leqq x \\leqq 3$ に含まれるので、最小値は頂点でとる。最大値は定義域の端のうち軸から遠い方 $x=3$ でとる。",
        "標準",
        &["最大・最小", "定義域"],
    )
    .map_err(err_str)?;

    insert_problem(
        &conn,
        u_hanbetsu,
        "判別式と解の個数",
        "二次方程式 $x^2 + 2kx + k + 2 = 0$ が異なる2つの実数解をもつとき、定数 $k$ の値の範囲を求めよ。",
        "判別式を $D$ とすると\n\\[ \\frac{D}{4} = k^2 - (k + 2) = k^2 - k - 2 = (k-2)(k+1) \\]\n異なる2つの実数解をもつ条件は $D > 0$ であるから\n\\[ (k-2)(k+1) > 0 \\]\nよって $k < -1,\\ 2 < k$。",
        "$x^2$ の係数が1、$x$ の係数が $2kx$ と偶数の形なので $D/4$ を使うと計算が簡単になる。",
        "標準",
        &["判別式", "解の個数"],
    )
    .map_err(err_str)?;

    insert_problem(
        &conn,
        u_hanbetsu,
        "常に正となる条件",
        "すべての実数 $x$ に対して $x^2 + 2ax + 4 > 0$ が成り立つような定数 $a$ の値の範囲を求めよ。",
        "$x^2$ の係数は正なので、条件は判別式 $D < 0$。\n\\[ \\frac{D}{4} = a^2 - 4 < 0 \\]\nよって $-2 < a < 2$。",
        "下に凸の放物線が常に $x$ 軸より上にある条件は、$x$ 軸と共有点をもたないこと、すなわち $D < 0$ である。グラフのイメージを持つことが大切。",
        "発展",
        &["判別式", "不等式", "絶対不等式"],
    )
    .map_err(err_str)?;

    insert_problem(
        &conn,
        u_baai,
        "順列の基本",
        "男子4人、女子3人が1列に並ぶとき、次の並び方は何通りあるか。\n\\begin{enumerate}\n\\item 7人が自由に並ぶ。\n\\item 女子3人が隣り合う。\n\\end{enumerate}",
        "\\begin{enumerate}\n\\item $7! = 5040$（通り）\n\\item 女子3人をひとまとめにすると5つのものの順列で $5!$、女子内部の並びが $3!$。\n\\[ 5! \\times 3! = 120 \\times 6 = 720 \\text{（通り）} \\]\n\\end{enumerate}",
        "「隣り合う」ものはひとまとめにして考えるのが定石。まとめた内部の並べ方を掛け合わせることを忘れないこと。",
        "基礎",
        &["順列", "場合の数"],
    )
    .map_err(err_str)?;

    Ok(())
}

pub fn has_any_data(state: &AppState) -> Result<bool, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM subjects", [], |r| r.get(0))
        .map_err(err_str)?;
    Ok(n > 0)
}
