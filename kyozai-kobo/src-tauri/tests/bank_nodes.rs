use kyozai_kobo_lib::commands::{bank, problems, projects, tree};
use kyozai_kobo_lib::db;
use kyozai_kobo_lib::models::SearchQuery;
use kyozai_kobo_lib::state::AppState;
use std::sync::Arc;

fn setup() -> (tempdir::TempDir, Arc<AppState>) {
    let dir = tempdir::TempDir::new("kyozai-bank-nodes").unwrap();
    let conn = db::open_db(dir.path()).unwrap();
    let state = Arc::new(AppState::new(conn, dir.path().to_path_buf()));
    (dir, state)
}

fn empty_search(bank_node_id: Option<i64>) -> SearchQuery {
    SearchQuery {
        text: String::new(),
        bank_node_id,
        include_descendants: Some(true),
        subject_id: None,
        field_id: None,
        unit_id: None,
        difficulty: None,
        difficulty_rank: None,
        difficulty_ranks: None,
        required_filter: None,
        tag: None,
    }
}

#[test]
fn v12_migration_preserves_legacy_problem_id_and_membership() {
    let dir = tempdir::TempDir::new("kyozai-bank-migration").unwrap();
    {
        let conn = db::open_db(dir.path()).unwrap();
        conn.execute(
            "INSERT INTO subjects(id,name,sort_order) VALUES (10,'数学',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO fields(id,subject_id,name,sort_order) VALUES (20,10,'数III',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO units(id,field_id,name,sort_order) VALUES (30,20,'積分',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO problems(id,unit_id,title,created_at,updated_at)
             VALUES (77,30,'既存問題','2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects(id,name,created_at,updated_at) VALUES (5,'移行確認','2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_items(project_id,item_type,sort_order,problem_id,snap_title,created_at)
             VALUES (5,'problem',1,77,'既存問題','2026-01-01')",
            [],
        )
        .unwrap();
        // v11相当の状態へ戻し、open_dbのv12 migrationを通す。
        conn.execute("UPDATE problems SET bank_node_id=NULL", [])
            .unwrap();
        conn.execute("DELETE FROM bank_nodes", []).unwrap();
        conn.execute_batch("PRAGMA user_version=11;").unwrap();
    }

    let conn = db::open_db(dir.path()).unwrap();
    let (problem_id, unit_id, bank_node_id): (i64, i64, i64) = conn
        .query_row(
            "SELECT id,unit_id,bank_node_id FROM problems WHERE id=77",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(problem_id, 77);
    assert_eq!(unit_id, 30);
    let path = problems::bank_path(&conn, bank_node_id).unwrap();
    assert_eq!(path, "数学 / 数III / 積分");
    let project_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM project_items WHERE problem_id=77",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(project_refs, 1);
}

#[test]
fn arbitrary_hierarchy_problem_move_search_and_safe_delete() {
    let (_dir, state) = setup();
    let root = tree::create_bank_node(&state, None, "数学".into()).unwrap();
    let level2 = tree::create_bank_node(&state, Some(root), "数III".into()).unwrap();
    let level3 = tree::create_bank_node(&state, Some(level2), "積分".into()).unwrap();
    let level4 = tree::create_bank_node(&state, Some(level3), "積分評価".into()).unwrap();
    let level5 = tree::create_bank_node(&state, Some(level4), "接線利用".into()).unwrap();
    let level6 = tree::create_bank_node(&state, Some(level5), "発展".into()).unwrap();

    let root_problem =
        problems::create_problem_in_bank_node(&state, root, "root問題".into()).unwrap();
    let deep_problem =
        problems::create_problem_in_bank_node(&state, level6, "deep問題".into()).unwrap();
    let project_id = projects::create_project(&state, "参照維持".into(), None).unwrap();
    projects::add_problem_to_project(&state, project_id, deep_problem).unwrap();
    assert_eq!(
        problems::list_problems_in_bank_node(&state, root)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        problems::list_problems_in_bank_node(&state, level6)
            .unwrap()
            .len(),
        1
    );

    let scoped = problems::search_problems(&state, empty_search(Some(level3))).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].id, deep_problem);
    assert_eq!(
        scoped[0].bank_path,
        "数学 / 数III / 積分 / 積分評価 / 接線利用 / 発展"
    );

    bank::move_problems_to_bank_node(&state, vec![deep_problem], level3).unwrap();
    assert_eq!(
        problems::get_problem(&state, deep_problem)
            .unwrap()
            .bank_node_id,
        level3
    );
    assert_eq!(
        projects::get_project(&state, project_id).unwrap().items[0].problem_id,
        Some(deep_problem)
    );
    assert_eq!(
        problems::get_problem(&state, root_problem).unwrap().id,
        root_problem
    );

    tree::rename_bank_node(&state, level4, "評価".into()).unwrap();
    let sibling = tree::create_bank_node(&state, Some(level3), "演習".into()).unwrap();
    tree::reorder_bank_node(&state, sibling, -1).unwrap();
    tree::move_bank_node(&state, sibling, Some(level2), None).unwrap();
    assert!(tree::move_bank_node(&state, level2, Some(level6), None).is_err());

    let temporary = tree::create_bank_node(&state, Some(level3), "一時分類".into()).unwrap();
    let moved_on_delete =
        problems::create_problem_in_bank_node(&state, temporary, "退避".into()).unwrap();
    let impact = tree::get_bank_node_delete_impact(&state, temporary).unwrap();
    assert_eq!(impact.descendant_problem_count, 1);
    tree::delete_bank_node(&state, temporary, "move_to_parent".into()).unwrap();
    assert_eq!(
        problems::get_problem(&state, moved_on_delete)
            .unwrap()
            .bank_node_id,
        level3
    );

    let doomed = tree::create_bank_node(&state, Some(level3), "削除対象".into()).unwrap();
    let doomed_child = tree::create_bank_node(&state, Some(doomed), "子".into()).unwrap();
    let doomed_problem =
        problems::create_problem_in_bank_node(&state, doomed_child, "削除問題".into()).unwrap();
    tree::delete_bank_node(&state, doomed, "delete_all".into()).unwrap();
    assert!(problems::get_problem(&state, doomed_problem).is_err());
}

#[test]
fn v2_export_import_preserves_arbitrary_depth() {
    let (dir_a, state_a) = setup();
    let mut parent = None;
    let mut deepest = 0;
    for name in ["大学別", "京都大学", "2026", "第3問系", "発展"] {
        deepest = tree::create_bank_node(&state_a, parent, name.into()).unwrap();
        parent = Some(deepest);
    }
    problems::create_problem_in_bank_node(&state_a, deepest, "京大問題".into()).unwrap();
    let export = {
        let conn = state_a.conn.lock().unwrap();
        bank::build_bank_export_v2(&conn, &dir_a.path().join("attachments"), "all", None, None)
            .unwrap()
    };
    assert_eq!(export.format_version, 2);
    assert_eq!(
        export.nodes[0].children[0].children[0].children[0].children[0]
            .problems
            .len(),
        1
    );

    let (dir_b, state_b) = setup();
    let result = {
        let conn = state_b.conn.lock().unwrap();
        bank::apply_bank_import_v2(&conn, &dir_b.path().join("attachments"), &export).unwrap()
    };
    assert_eq!(result.nodes_created, 5);
    assert_eq!(result.problems_imported, 1);
    let found = problems::search_problems(
        &state_b,
        SearchQuery {
            text: "京大問題".into(),
            ..empty_search(None)
        },
    )
    .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].bank_path,
        "大学別 / 京都大学 / 2026 / 第3問系 / 発展"
    );
}
