use kyozai_kobo_lib::commands::patterns;
use kyozai_kobo_lib::db;
use kyozai_kobo_lib::models::{
    ApplyPatternProposalPayload, PatternExtractionResult, PatternFacets, PatternProposal,
    PatternProposalStrategy, PatternSearchQuery, PatternStrategyInput, PatternUpdate,
};
use kyozai_kobo_lib::state::AppState;
use rusqlite::params;
use std::sync::Arc;

fn make_state() -> (tempdir::TempDir, Arc<AppState>) {
    let dir = tempdir::TempDir::new("kyozai-pattern-test").unwrap();
    let conn = db::open_db(dir.path()).unwrap();
    let data_dir = dir.path().to_path_buf();
    (dir, Arc::new(AppState::new(conn, data_dir)))
}

fn seed_problem(state: &AppState, title: &str) -> i64 {
    let conn = state.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO subjects(name,sort_order) VALUES ('数学',1)",
        [],
    )
    .unwrap();
    let subject_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fields(subject_id,name,sort_order) VALUES (?1,'数III',1)",
        params![subject_id],
    )
    .unwrap();
    let field_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO units(field_id,name,sort_order) VALUES (?1,'積分',1)",
        params![field_id],
    )
    .unwrap();
    let unit_id = conn.last_insert_rowid();
    let now = db::now_str();
    conn.execute(
        "INSERT INTO problems(unit_id,title,statement_latex,created_at,updated_at)
         VALUES (?1,?2,'\\(\\int_0^1 f(x)dx\\)',?3,?3)",
        params![unit_id, title, now],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn pattern_update(id: i64, version: i64, title: &str) -> PatternUpdate {
    PatternUpdate {
        id,
        expected_version: Some(version),
        title: title.into(),
        summary: "任意の実数xで成立する条件を整理する".into(),
        pattern_type: "strategy".into(),
        situation: "パラメータを含む式が任意のxで成立する。".into(),
        principle: "恒等式・最大最小・特殊値の観点から候補を比較する。".into(),
        cautions: "特殊値代入だけでは十分条件にならない場合がある。".into(),
        examples: "\\(ax+b=0\\) が任意の \\(x\\) で成立する場合。".into(),
        source_note: "自分で作成".into(),
        tags: vec!["恒等式".into(), "パラメータ".into()],
        facets: PatternFacets {
            domains: vec!["数と式".into()],
            goals: vec!["存在条件".into()],
            operations: vec!["代入".into(), "微分".into()],
            structures: vec!["恒等性".into()],
            situations: vec!["任意のx".into()],
        },
        strategies: vec![
            PatternStrategyInput {
                id: None,
                parent_strategy_id: None,
                title: "係数比較".into(),
                description: "次数ごとの係数を見る。".into(),
                condition: "整式の恒等式として扱える。".into(),
                reasoning: "各次数の係数が一致する必要がある。".into(),
                branch_label: String::new(),
                sort_order: 0,
            },
            PatternStrategyInput {
                id: None,
                parent_strategy_id: None,
                title: "特殊値代入".into(),
                description: "x=0,±1などを試す。".into(),
                condition: "必要条件を効率よく抽出できる値がある。".into(),
                reasoning: "任意のxなら特定のxでも成立する。".into(),
                branch_label: String::new(),
                sort_order: 1,
            },
        ],
    }
}

fn proposal(title: &str, source_type: &str) -> PatternProposal {
    PatternProposal {
        proposal_id: "proposal-test".into(),
        title: title.into(),
        pattern_type: "strategy".into(),
        summary: "凸性を利用して定積分の値を評価する。".into(),
        situation: "凸関数を含む定積分を上からまたは下から評価したい。".into(),
        principle: "接線とグラフの上下関係を積分可能な不等式へ変換する。".into(),
        strategies: vec![PatternProposalStrategy {
            title: "接線を引く".into(),
            description: "比較しやすい点で接線を作る。".into(),
            condition: "対象関数の凸性と接点を確認できる。".into(),
            reasoning: "凸関数の接線はグラフの下側にある。".into(),
            sort_order: 1,
        }],
        cautions: vec!["凸性の向きと不等号の向きを確認する。".into()],
        domains: vec!["積分".into()],
        goals: vec!["評価".into()],
        operations: vec!["積分".into()],
        structures: vec!["凸性".into()],
        situations: vec!["定積分評価".into()],
        tags: vec!["接線".into()],
        source_type: source_type.into(),
        matched_pattern_id: None,
        matched_pattern_title: None,
        similarity_reason: String::new(),
        action_recommendation: "create_new".into(),
        raw_technique: r"\(y=e^x\)のグラフの点(0,1)での接線を引き、区間で積分する。".into(),
        generalization_reason: "特定の関数と区間を外し、凸性と接線の上下関係だけを残した。".into(),
        specificity_level: 2,
        reusability_score: 0.8,
        search_concepts: vec!["凸性".into(), "接線".into(), "不等式評価".into()],
        is_overly_specific: false,
        is_overly_general: false,
        specificity_reason: String::new(),
        possible_parent_pattern: None,
        generalization_decision: "keep_as_is".into(),
        recommended_storage: "new_pattern".into(),
        generalization_pass_count: 0,
    }
}

#[test]
fn migration_adds_pattern_schema_without_losing_existing_assets() {
    let dir = tempdir::TempDir::new("kyozai-pattern-migration").unwrap();
    let conn = db::open_db(dir.path()).unwrap();
    conn.execute(
        "INSERT INTO subjects(name,sort_order) VALUES ('数学',1)",
        [],
    )
    .unwrap();
    let subject_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fields(subject_id,name,sort_order) VALUES (?1,'数I',1)",
        params![subject_id],
    )
    .unwrap();
    let field_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO units(field_id,name,sort_order) VALUES (?1,'二次関数',1)",
        params![field_id],
    )
    .unwrap();
    let unit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO problems(id,unit_id,title,created_at,updated_at) VALUES (91,?1,'既存問題','2026-01-01','2026-01-01')",
        params![unit_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects(id,name,created_at,updated_at) VALUES (92,'既存教材','2026-01-01','2026-01-01')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "DROP TABLE pattern_versions;
         DROP TABLE pattern_relations;
         DROP TABLE problem_patterns;
         DROP TABLE pattern_facets;
         DROP TABLE pattern_tags;
         DROP TABLE pattern_strategies;
         DROP TABLE patterns;
         PRAGMA user_version=12;",
    )
    .unwrap();
    drop(conn);

    let conn = db::open_db(dir.path()).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        15
    );
    assert_eq!(
        conn.query_row("SELECT title FROM problems WHERE id=91", [], |row| row
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "既存問題"
    );
    assert_eq!(
        conn.query_row("SELECT name FROM projects WHERE id=92", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "既存教材"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM patterns", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn pattern_crud_candidates_filters_relations_versions_and_safe_delete() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "関連問題");
    let id = patterns::create_pattern(&state, "任意のxで成立".into(), "strategy".into()).unwrap();
    assert_eq!(
        patterns::update_pattern(&state, pattern_update(id, 1, "任意のxで成立する条件")).unwrap(),
        2
    );

    let full = patterns::get_pattern(&state, id).unwrap();
    assert_eq!(full.strategies.len(), 2);
    assert_eq!(full.strategies[0].title, "係数比較");
    assert_eq!(full.tags, vec!["パラメータ", "恒等式"]);
    assert_eq!(full.facets.operations, vec!["代入", "微分"]);

    let by_title = patterns::search_patterns(
        &state,
        PatternSearchQuery {
            text: "任意のx".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_title.len(), 1);
    let by_candidate = patterns::search_patterns(
        &state,
        PatternSearchQuery {
            text: "特殊値".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_candidate.len(), 1);
    let by_tag = patterns::search_patterns(
        &state,
        PatternSearchQuery {
            tag: Some("恒等式".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_tag.len(), 1);
    let by_facet = patterns::search_patterns(
        &state,
        PatternSearchQuery {
            operation: Some("微分".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_facet.len(), 1);

    let other =
        patterns::create_pattern(&state, "特殊値を代入する".into(), "technique".into()).unwrap();
    patterns::link_pattern_relation(&state, id, other, "related".into()).unwrap();
    patterns::link_pattern_relation(&state, id, other, "related".into()).unwrap();
    assert_eq!(
        patterns::get_pattern(&state, id)
            .unwrap()
            .related_patterns
            .len(),
        1
    );

    patterns::link_problem_pattern(&state, problem_id, id, "applicable".into()).unwrap();
    assert_eq!(
        patterns::list_patterns_for_problem(&state, problem_id).unwrap()[0].relation_type,
        "applicable"
    );
    patterns::link_problem_pattern(&state, problem_id, id, "used".into()).unwrap();
    assert_eq!(
        patterns::list_patterns_for_problem(&state, problem_id).unwrap()[0].relation_type,
        "used"
    );

    let mut reorder = pattern_update(id, 2, "並び替え後の定石");
    reorder.strategies = full
        .strategies
        .iter()
        .rev()
        .map(|strategy| PatternStrategyInput {
            id: Some(strategy.id),
            parent_strategy_id: strategy.parent_strategy_id,
            title: strategy.title.clone(),
            description: strategy.description.clone(),
            condition: strategy.condition.clone(),
            reasoning: strategy.reasoning.clone(),
            branch_label: strategy.branch_label.clone(),
            sort_order: 0,
        })
        .collect();
    assert_eq!(patterns::update_pattern(&state, reorder).unwrap(), 3);
    assert_eq!(
        patterns::get_pattern(&state, id).unwrap().strategies[0].title,
        "特殊値代入"
    );

    let mut update = pattern_update(id, 3, "更新後の定石");
    let retained_id = full.strategies[1].id;
    update.tags = vec!["恒等式".into()];
    update.facets.operations = vec!["代入".into()];
    update.strategies = vec![PatternStrategyInput {
        id: Some(retained_id),
        parent_strategy_id: None,
        title: "特殊値代入（更新）".into(),
        description: "候補を絞る。".into(),
        condition: "有効な特殊値がある。".into(),
        reasoning: "必要条件を得る。".into(),
        branch_label: String::new(),
        sort_order: 0,
    }];
    assert_eq!(patterns::update_pattern(&state, update).unwrap(), 4);
    let updated = patterns::get_pattern(&state, id).unwrap();
    assert_eq!(updated.strategies.len(), 1);
    assert_eq!(updated.strategies[0].id, retained_id);
    assert_eq!(updated.tags, vec!["恒等式"]);
    let removed_facet = patterns::search_patterns(
        &state,
        PatternSearchQuery {
            operation: Some("微分".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(removed_facet.is_empty());

    let versions = patterns::list_pattern_versions(&state, id).unwrap();
    assert_eq!(versions.len(), 3);
    let version_three = versions
        .iter()
        .find(|version| version.version == 3)
        .unwrap();
    let preview = patterns::get_pattern_version(&state, version_three.id).unwrap();
    assert_eq!(preview.snapshot.strategies.len(), 2);
    assert_eq!(
        patterns::restore_pattern_version(&state, version_three.id, Some(4)).unwrap(),
        5
    );
    assert_eq!(
        patterns::get_pattern(&state, id).unwrap().strategies.len(),
        2
    );

    let impact = patterns::get_pattern_delete_impact(&state, id).unwrap();
    assert_eq!(impact.problem_count, 1);
    assert_eq!(impact.related_pattern_count, 1);
    patterns::unlink_pattern_relation(&state, id, other, "related".into()).unwrap();
    patterns::unlink_problem_pattern(&state, problem_id, id).unwrap();
    patterns::delete_pattern(&state, id).unwrap();
    let conn = state.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM problems WHERE id=?1",
            params![problem_id],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn pattern_json_round_trip_preserves_content_and_relations() {
    let (_source_dir, source) = make_state();
    let source_problem = seed_problem(&source, "同一関連問題");
    let first =
        patterns::create_pattern(&source, "接線による積分評価".into(), "strategy".into()).unwrap();
    patterns::update_pattern(&source, pattern_update(first, 1, "接線による積分評価")).unwrap();
    let second =
        patterns::create_pattern(&source, "凸性の利用".into(), "technique".into()).unwrap();
    patterns::link_pattern_relation(&source, first, second, "prerequisite".into()).unwrap();
    patterns::link_problem_pattern(&source, source_problem, first, "used".into()).unwrap();
    let json = patterns::export_patterns_json(&source, None).unwrap();
    assert!(json.contains("kyozai-kobo-pattern-library"));
    assert!(json.contains("\"format_version\": 1"));

    let (_target_dir, target) = make_state();
    let target_problem = seed_problem(&target, "同一関連問題");
    assert_eq!(source_problem, target_problem);
    let result = patterns::import_patterns_json(&target, json.clone()).unwrap();
    assert_eq!(result.created, 2);
    assert_eq!(result.relations_created, 1);
    assert_eq!(result.problem_relations_created, 1);
    let found = patterns::search_patterns(
        &target,
        PatternSearchQuery {
            text: "接線".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(found.len(), 1);
    let restored = patterns::get_pattern(&target, found[0].id).unwrap();
    assert_eq!(restored.strategies.len(), 2);
    assert_eq!(restored.related_patterns[0].relation_type, "prerequisite");
    assert_eq!(restored.related_problems[0].relation_type, "used");

    let duplicate = patterns::import_patterns_json(&target, json).unwrap();
    assert_eq!(duplicate.created, 0);
    assert_eq!(duplicate.skipped, 2);
    assert!(patterns::import_patterns_json(&target, "{not-json".into()).is_err());
}

#[test]
fn extraction_proposals_are_classified_and_only_apply_after_approval() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "積分評価");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE problems SET answer_latex='凸性を確認して接線と比較する。' WHERE id=?1",
            params![problem_id],
        )
        .unwrap();
    }
    let existing =
        patterns::create_pattern(&state, "接線による積分評価".into(), "strategy".into()).unwrap();
    let mut existing_update = pattern_update(existing, 1, "接線による積分評価");
    existing_update.summary = "凸性を利用して定積分の値を評価する。".into();
    existing_update.situation = "凸関数を含む定積分を上からまたは下から評価したい。".into();
    existing_update.strategies = vec![PatternStrategyInput {
        id: None,
        parent_strategy_id: None,
        title: "接線を引く".into(),
        description: "比較しやすい点で接線を作る。".into(),
        condition: "対象関数の凸性と接点を確認できる。".into(),
        reasoning: "凸関数の接線はグラフの下側にある。".into(),
        branch_label: String::new(),
        sort_order: 1,
    }];
    patterns::update_pattern(&state, existing_update).unwrap();

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![proposal("接線による積分評価", "solution_used")],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_eq!(extraction.patterns[0].matched_pattern_id, Some(existing));
    assert_eq!(extraction.patterns[0].action_recommendation, "duplicate");
    let count_before: i64 = state
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_before, 1, "分類だけではcanonical Patternを作成しない");

    let mut fresh = proposal("接線と面積評価", "solution_used");
    fresh.strategies[0].condition = "\\(0\\le x\\le 1\\)で凸性を確認できる。".into();
    let applied = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: fresh,
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: Some("used".into()),
            parent_pattern_id: None,
        },
    )
    .unwrap();
    assert!(applied.created && applied.linked);
    let created = patterns::get_pattern(&state, applied.pattern_id).unwrap();
    assert_eq!(created.strategies[0].title, "接線を引く");
    assert!(created.strategies[0].condition.contains("\\leqq"));
    assert!(created
        .source_note
        .contains(&format!("Problem #{}", problem_id)));
    assert_eq!(
        patterns::list_patterns_for_problem(&state, problem_id).unwrap()[0].relation_type,
        "used"
    );
}

#[test]
fn approved_existing_proposal_creates_version_and_invalid_provenance_rolls_back() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "積分評価");
    let existing =
        patterns::create_pattern(&state, "積分評価の基本".into(), "strategy".into()).unwrap();
    let original = patterns::get_pattern(&state, existing).unwrap();
    let mut candidate = proposal("積分評価の基本", "ai_inferred");
    candidate.matched_pattern_id = Some(existing);
    candidate.strategies[0].title = "区分求積と比較する".into();
    let result = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: candidate,
            action: "add_candidate_to_existing".into(),
            target_pattern_id: Some(existing),
            link_relation_type: Some("applicable".into()),
            parent_pattern_id: None,
        },
    )
    .unwrap();
    assert!(!result.created && result.linked);
    let updated = patterns::get_pattern(&state, existing).unwrap();
    assert_eq!(
        updated.title, original.title,
        "AI追記でcanonical titleを上書きしない"
    );
    assert_eq!(updated.version, original.version + 1);
    assert_eq!(updated.strategies.len(), 1);
    assert_eq!(
        patterns::list_pattern_versions(&state, existing)
            .unwrap()
            .len(),
        1
    );

    let count_before: i64 = state
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0))
        .unwrap();
    let error = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: proposal("解説由来と偽装", "explanation_used"),
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: None,
            parent_pattern_id: None,
        },
    )
    .expect_err("解説がないProblemを解説由来として保存できない");
    assert!(error.contains("解説がない"));
    let count_after: i64 = state
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_after, count_before);

    // カードへ出る本文の用語は、ユーザーが編集した後でも保存させない。
    let mut uncommon = proposal("大学用語を含む候補", "ai_inferred");
    uncommon.strategies[0].description = "勾配とヘッセ行列を調べる。".into();
    let language_error = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: uncommon,
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: None,
            parent_pattern_id: None,
        },
    )
    .expect_err("ユーザー編集後も高校数学で一般的でない用語を保存しない");
    assert!(language_error.contains("高校数学で一般的"));
}

/// 生成されたtitleそのものは既存定石と一致しなくても、
/// AIが返した一般化キーワードで上位の既存定石へ到達できることを確認する。
#[test]
fn search_concepts_reach_a_more_general_existing_pattern() {
    let (_dir, state) = make_state();
    let general = patterns::create_pattern(
        &state,
        "媒介変数表示を存在条件として扱う".into(),
        "strategy".into(),
    )
    .unwrap();
    let mut update = pattern_update(general, 1, "媒介変数表示を存在条件として扱う");
    update.summary =
        "媒介変数で表された点の集合を、条件を満たす媒介変数の存在条件へ言い換える。".into();
    update.situation = "媒介変数で表された点が動く範囲を求めたい。".into();
    update.tags = vec!["媒介変数".into(), "存在条件".into()];
    update.facets = PatternFacets {
        domains: vec!["軌跡と領域".into()],
        goals: vec!["存在条件".into()],
        operations: vec!["文字消去".into()],
        structures: vec!["媒介変数".into()],
        situations: vec!["通過領域".into()],
    };
    update.strategies = vec![PatternStrategyInput {
        id: None,
        parent_strategy_id: None,
        title: "媒介変数の存在条件へ言い換える".into(),
        description: "媒介変数を残したまま条件を整理する。".into(),
        condition: "媒介変数の動く範囲が定まっている。".into(),
        reasoning: "点の存在は媒介変数の存在と同値である。".into(),
        branch_label: String::new(),
        sort_order: 1,
    }];
    patterns::update_pattern(&state, update).unwrap();

    // titleは既存定石と重ならないが、searchConceptsで到達できる想定の候補。
    let mut candidate = proposal("内分表示から通過範囲を求める", "solution_used");
    candidate.summary =
        "内分表示された点の動く範囲を、媒介変数の条件へ言い換えて求める。".into();
    candidate.situation = "内分表示された点が動く範囲を求めたい。".into();
    candidate.search_concepts = vec!["媒介変数".into(), "存在条件".into(), "文字消去".into()];
    candidate.tags = vec!["媒介変数".into()];
    candidate.domains = vec!["軌跡と領域".into()];
    candidate.operations = vec!["文字消去".into()];
    candidate.structures = vec!["媒介変数".into()];
    candidate.situations = vec!["通過領域".into()];
    candidate.goals = vec!["存在条件".into()];
    candidate.strategies = vec![PatternProposalStrategy {
        title: "媒介変数の存在条件へ言い換える".into(),
        description: "媒介変数を残したまま条件を整理する。".into(),
        condition: "媒介変数の動く範囲が定まっている。".into(),
        reasoning: "点の存在は媒介変数の存在と同値である。".into(),
        sort_order: 1,
    }];
    candidate.recommended_storage = "example".into();

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![candidate],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_eq!(extraction.patterns[0].matched_pattern_id, Some(general));
    assert_eq!(
        extraction.patterns[0].action_recommendation, "add_example_to_existing",
        "既存の一般定石があるとき、新規作成より具体例追加を推奨する"
    );
}

/// 具体例として既存定石へ追記した場合、examplesへ残り、版も保存されることを確認する。
#[test]
fn example_action_appends_raw_technique_and_saves_version() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "通過領域の問題");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE problems SET answer_latex='内分表示して媒介変数を消去する。' WHERE id=?1",
            params![problem_id],
        )
        .unwrap();
    }
    let general = patterns::create_pattern(
        &state,
        "媒介変数表示を存在条件として扱う".into(),
        "strategy".into(),
    )
    .unwrap();
    patterns::update_pattern(
        &state,
        pattern_update(general, 1, "媒介変数表示を存在条件として扱う"),
    )
    .unwrap();
    let before = patterns::get_pattern(&state, general).unwrap();
    let versions_before = patterns::list_pattern_versions(&state, general).unwrap().len();

    let mut candidate = proposal("内分表示から通過範囲を求める", "solution_used");
    candidate.raw_technique = "線分上の点を内分表示し、媒介変数を消去する。".into();
    candidate.matched_pattern_id = Some(general);
    let applied = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: candidate,
            action: "add_example_to_existing".into(),
            target_pattern_id: Some(general),
            link_relation_type: Some("used".into()),
            parent_pattern_id: None,
        },
    )
    .unwrap();
    assert!(!applied.created && applied.linked);

    let after = patterns::get_pattern(&state, general).unwrap();
    assert!(after.examples.contains("媒介変数を消去する"));
    assert!(after.examples.contains(&before.examples));
    assert_eq!(after.version, before.version + 1);
    assert_eq!(
        patterns::list_pattern_versions(&state, general)
            .unwrap()
            .len(),
        versions_before + 1,
        "追記の前に変更履歴を保存する"
    );
}

/// 特殊化として保存した場合、新規定石が作られ、上位・下位の関連が張られることを確認する。
#[test]
fn child_pattern_action_creates_specialization_relations() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "回転体の体積");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE problems SET answer_latex='断面を円板として積分する。' WHERE id=?1",
            params![problem_id],
        )
        .unwrap();
    }
    let parent = patterns::create_pattern(
        &state,
        "立体を断面積の積分として捉える".into(),
        "strategy".into(),
    )
    .unwrap();
    patterns::update_pattern(
        &state,
        pattern_update(parent, 1, "立体を断面積の積分として捉える"),
    )
    .unwrap();

    let parent_before = patterns::get_pattern(&state, parent).unwrap();

    let mut candidate = proposal("回転体を円板・円環断面として積分する", "solution_used");
    candidate.matched_pattern_id = Some(parent);
    let applied = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal: candidate,
            action: "create_child_pattern".into(),
            target_pattern_id: None,
            link_relation_type: Some("used".into()),
            parent_pattern_id: Some(parent),
        },
    )
    .unwrap();
    assert!(applied.created);
    assert_ne!(applied.pattern_id, parent);

    let child = patterns::get_pattern(&state, applied.pattern_id).unwrap();
    assert!(
        child.related_patterns.iter().any(|relation| {
            relation.pattern_id == parent && relation.relation_type == "generalization"
        }),
        "下位から上位へはgeneralization"
    );
    let parent_view = patterns::get_pattern(&state, parent).unwrap();
    assert!(
        parent_view.related_patterns.iter().any(|relation| {
            relation.pattern_id == applied.pattern_id
                && relation.relation_type == "specialization"
        }),
        "上位から下位へはspecialization"
    );
    assert_eq!(
        parent_view.version, parent_before.version,
        "特殊化の作成では上位定石の本文を書き換えない"
    );
}

/// 抽象化に関する項目を持たない旧ジョブのProposalでも、そのまま保存できることを確認する。
#[test]
fn legacy_proposal_without_generalization_fields_can_be_applied() {
    let (_dir, state) = make_state();
    let problem_id = seed_problem(&state, "旧形式の問題");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE problems SET answer_latex='接線と比較する。' WHERE id=?1",
            params![problem_id],
        )
        .unwrap();
    }
    let legacy = serde_json::json!({
        "proposalId": "proposal-1",
        "title": "凸性を利用して定積分を評価する",
        "patternType": "strategy",
        "summary": "凸性を利用して定積分の値を評価する。",
        "situation": "凸関数を含む定積分を上からまたは下から評価したい。",
        "principle": "接線とグラフの上下関係を積分可能な不等式へ変換する。",
        "strategies": [{
            "title": "接線を引く",
            "description": "比較しやすい点で接線を作る。",
            "condition": "対象関数の凸性と接点を確認できる。",
            "reasoning": "凸関数の接線はグラフの下側にある。",
            "sortOrder": 1
        }],
        "cautions": [],
        "domains": [],
        "goals": [],
        "operations": [],
        "structures": [],
        "situations": [],
        "tags": [],
        "sourceType": "solution_used",
        "matchedPatternId": null,
        "matchedPatternTitle": null,
        "similarityReason": "",
        "actionRecommendation": "create_new"
    });
    let proposal: PatternProposal = serde_json::from_value(legacy).unwrap();
    assert_eq!(proposal.specificity_level, 0, "旧形式では抽象度を持たない");
    let applied = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id,
            source_reference: String::new(),
            proposal,
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: None,
            parent_pattern_id: None,
        },
    )
    .unwrap();
    assert!(applied.created);
}

/// 実際のCodex出力で起きた取りこぼしの回帰テスト。
/// タイトルの字面が違っても、概念語と語の重なりで上位の既存定石へ到達できること。
#[test]
fn generalized_title_still_matches_the_seeded_parent_pattern() {
    let (_dir, state) = make_state();
    let parent = patterns::create_pattern(
        &state,
        "立体の体積を求めるために断面積を積分する".into(),
        "strategy".into(),
    )
    .unwrap();
    let mut update = pattern_update(parent, 1, "立体の体積を求めるために断面積を積分する");
    update.summary =
        "立体の体積は、ある方向に垂直な平面で切った断面積を、その方向について積分して求める。"
            .into();
    update.situation = "立体の体積を求めたく、切り口の面積を位置の関数として表せるとき。".into();
    update.principle =
        "断面積を位置の関数として表し、立体が存在する範囲でその関数を積分すれば体積が得られる。"
            .into();
    update.tags = vec!["体積".into(), "断面積".into(), "積分".into()];
    update.facets = PatternFacets {
        domains: vec!["微分と積分".into()],
        goals: vec!["体積の決定".into()],
        operations: vec!["積分".into(), "断面の把握".into()],
        structures: vec!["立体".into(), "断面".into()],
        situations: vec!["体積を求める".into()],
    };
    update.strategies = vec![PatternStrategyInput {
        id: None,
        parent_strategy_id: None,
        title: "切る方向を決めて断面積を位置の関数で表す".into(),
        description: "断面積を位置の関数として表す。".into(),
        condition: "切り口の面積を位置の式で書けるとき。".into(),
        reasoning: "体積は断面積の積分として表せる。".into(),
        branch_label: String::new(),
        sort_order: 1,
    }];
    patterns::update_pattern(&state, update).unwrap();

    // 実際のAI出力と同じ形。タイトルは既存定石の部分文字列にならない。
    let mut candidate = proposal(
        "断面積が位置の式で表される立体では、切断位置について積分して体積を求める",
        "solution_used",
    );
    candidate.summary =
        "断面積を位置の関数として表せる立体では、切断位置について積分して体積を求める。".into();
    candidate.situation = "立体の体積を求めたく、断面積を位置の式で表せるとき。".into();
    candidate.principle = "体積は断面積を切断位置について積分した値に等しい。".into();
    candidate.search_concepts = vec![
        "区分求積".into(),
        "微小量の和".into(),
        "断面積".into(),
        "体積".into(),
        "積分区間".into(),
        "立体の切断".into(),
    ];
    candidate.tags = vec!["断面積".into(), "体積".into()];
    candidate.domains = vec!["微分と積分".into()];
    candidate.goals = vec!["体積の決定".into()];
    candidate.operations = vec!["積分".into()];
    candidate.structures = vec!["立体".into()];
    candidate.situations = vec!["体積を求める".into()];
    candidate.strategies = vec![PatternProposalStrategy {
        title: "切断位置を変数にして断面積を表す".into(),
        description: "断面積を位置の関数として書き下す。".into(),
        condition: "断面の形が位置から決まるとき。".into(),
        reasoning: "体積が一変数の積分に帰着する。".into(),
        sort_order: 1,
    }];
    candidate.recommended_storage = "child_pattern".into();

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![candidate],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_eq!(
        extraction.patterns[0].matched_pattern_id,
        Some(parent),
        "タイトルの言い回しが違っても上位の既存定石を見つける"
    );
    assert_eq!(
        extraction.patterns[0].action_recommendation, "create_child_pattern",
        "上位が既にあるなら新規作成ではなく特殊化を推奨する"
    );
}

/// 語が少し重なるだけの無関係な定石は、既存候補として拾わないことを確認する。
#[test]
fn unrelated_pattern_is_not_matched_by_a_single_shared_word() {
    let (_dir, state) = make_state();
    let unrelated =
        patterns::create_pattern(&state, "余事象を考えて確率を求める".into(), "strategy".into())
            .unwrap();
    let mut update = pattern_update(unrelated, 1, "余事象を考えて確率を求める");
    update.summary = "求めにくい確率は、余事象の確率を1から引いて求める。".into();
    update.situation = "少なくとも1つという条件の確率を求めたいとき。".into();
    update.principle = "全体から余事象を除けば目的の確率が得られる。".into();
    update.tags = vec!["確率".into()];
    update.facets = PatternFacets {
        domains: vec!["場合の数と確率".into()],
        goals: vec!["確率の決定".into()],
        operations: vec!["余事象".into()],
        structures: vec!["事象".into()],
        situations: vec!["少なくとも1つ".into()],
    };
    update.strategies = vec![PatternStrategyInput {
        id: None,
        parent_strategy_id: None,
        title: "余事象の確率を求める".into(),
        description: "反対の場合を数える。".into(),
        condition: "余事象の方が数えやすいとき。".into(),
        reasoning: "全体の確率は1である。".into(),
        branch_label: String::new(),
        sort_order: 1,
    }];
    patterns::update_pattern(&state, update).unwrap();

    let mut candidate = proposal(
        "断面積が位置の式で表される立体では、切断位置について積分して体積を求める",
        "solution_used",
    );
    candidate.summary = "断面積を位置の関数として表し、切断位置について積分する。".into();
    candidate.situation = "立体の体積を求めたいとき。".into();
    candidate.principle = "体積は断面積の積分に等しい。".into();
    candidate.search_concepts = vec!["断面積".into(), "体積".into(), "求める".into()];
    candidate.tags = vec!["断面積".into()];
    candidate.domains = vec!["微分と積分".into()];
    candidate.goals = vec!["体積の決定".into()];
    candidate.operations = vec!["積分".into()];
    candidate.structures = vec!["立体".into()];
    candidate.situations = vec!["体積を求める".into()];

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![candidate],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_eq!(
        extraction.patterns[0].matched_pattern_id, None,
        "分野の違う定石を語の偶然の一致で結び付けない"
    );
    assert_eq!(extraction.patterns[0].action_recommendation, "create_new");
}

/// AIが挙げた上位定石が既にライブラリにあるなら、語彙が重ならなくても特殊化として結び付ける。
/// 実際のCodex出力で、上位ヒントだけが手掛かりになる候補が取りこぼされた回帰テスト。
#[test]
fn parent_hint_reaches_the_existing_general_pattern() {
    let (_dir, state) = make_state();
    let parent = patterns::create_pattern(
        &state,
        "立体の体積を求めるために断面積を積分する".into(),
        "strategy".into(),
    )
    .unwrap();
    let mut update = pattern_update(parent, 1, "立体の体積を求めるために断面積を積分する");
    update.summary = "断面積を位置の関数として表し、その方向について積分して体積を求める。".into();
    update.situation = "立体の体積を求めたいとき。".into();
    update.principle = "体積は断面積の積分として表せる。".into();
    update.tags = vec!["体積".into()];
    update.facets = PatternFacets {
        domains: vec!["微分と積分".into()],
        goals: vec!["体積の決定".into()],
        operations: vec!["積分".into()],
        structures: vec!["立体".into()],
        situations: vec!["体積を求める".into()],
    };
    patterns::update_pattern(&state, update).unwrap();

    // 語彙は既存定石とほとんど重ならないが、上位定石のヒントだけが一致する候補。
    let mut candidate = proposal(
        "回転軸に垂直な断面が円になるとき、軸からの距離で断面積を作る",
        "solution_used",
    );
    candidate.summary = "回転体では、軸からの距離を半径として断面の面積を作る。".into();
    candidate.situation = "回転体の断面の形を決めたいとき。".into();
    candidate.principle = "回転軸に垂直な断面は円または円環になる。".into();
    candidate.search_concepts = vec!["回転体".into(), "円板".into(), "円環".into()];
    candidate.tags = vec!["回転体".into()];
    candidate.domains = vec!["図形と計量".into()];
    candidate.goals = vec!["断面の決定".into()];
    candidate.operations = vec!["半径の決定".into()];
    candidate.structures = vec!["回転体".into()];
    candidate.situations = vec!["回転体の断面".into()];
    candidate.recommended_storage = "child_pattern".into();
    candidate.possible_parent_pattern = Some(kyozai_kobo_lib::models::PatternParentHint {
        title: "断面積が求められる立体では、断面積を積分して体積を求める".into(),
        reason: "回転体はその特殊な場合である。".into(),
    });

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![candidate],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_eq!(
        extraction.patterns[0].matched_pattern_id,
        Some(parent),
        "上位定石のヒントだけでも既存定石へ到達する"
    );
    assert_eq!(
        extraction.patterns[0].action_recommendation, "create_child_pattern",
        "上位が既にあるなら特殊化として作成する"
    );
}

/// v14からの移行で、教材項目に定石用の列が増えても既存の教材内容が壊れないことを確認する。
#[test]
fn migration_from_v14_adds_pattern_item_columns_without_losing_materials() {
    let dir = tempdir::TempDir::new("kyozai-pattern-v15").unwrap();
    {
        // まず現行スキーマでDBを作り、教材と項目を入れる。
        let conn = db::open_db(dir.path()).unwrap();
        conn.execute(
            "INSERT INTO projects(id,name,description,created_at,updated_at) VALUES (71,'既存教材','',?1,?1)",
            params![db::now_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_items(id,project_id,item_type,sort_order,snap_title,content,created_at)
             VALUES (72,71,'text',0,'','既存の説明文',?1)",
            params![db::now_str()],
        )
        .unwrap();
        // v14相当へ戻す（列は残るが、移行が再実行されても壊れないことを見る）。
        conn.execute_batch("PRAGMA user_version=14;").unwrap();
    }

    let conn = db::open_db(dir.path()).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        15
    );
    assert_eq!(
        conn.query_row("SELECT content FROM project_items WHERE id=72", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "既存の説明文",
        "移行で既存の教材項目を失わない"
    );
    // 定石用の列が使える。
    conn.execute(
        "UPDATE project_items SET snap_pattern_json='{}' WHERE id=72",
        [],
    )
    .unwrap();
    let columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(project_items)").unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert!(columns.iter().any(|name| name == "pattern_id"));
    assert!(columns.iter().any(|name| name == "snap_pattern_json"));
    let pattern_columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(patterns)").unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert!(pattern_columns.iter().any(|name| name == "source_kind"));
}

/// 画像・AI Chat・手動由来の候補は、Problemに紐づかなくても保存できることを確認する。
#[test]
fn proposals_without_a_source_problem_can_be_saved_with_their_provenance() {
    let (_dir, state) = make_state();
    let mut candidate = proposal("関数値の差を導関数の範囲へ移して評価する", "image_import");
    candidate.raw_technique = "参考書のKEY 1に書かれていた内容。".into();
    let applied = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id: 0,
            source_reference: "sample-page.jpg".into(),
            proposal: candidate,
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: None,
            parent_pattern_id: None,
        },
    )
    .unwrap();
    assert!(applied.created && !applied.linked);

    let (source_note, source_kind): (String, String) = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT source_note,source_kind FROM patterns WHERE id=?1",
            params![applied.pattern_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(source_note.contains("画像から取り込み"));
    assert!(
        source_note.contains("sample-page.jpg"),
        "画像そのものではなくファイル名だけを由来として残す"
    );
    assert_eq!(source_kind, "image_import");
}

/// Problem由来を名乗る候補は、抽出元Problemなしでは保存できない。
#[test]
fn problem_based_source_types_still_require_their_problem() {
    let (_dir, state) = make_state();
    let error = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id: 0,
            source_reference: String::new(),
            proposal: proposal("接線で積分を評価する", "solution_used"),
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: None,
            parent_pattern_id: None,
        },
    )
    .unwrap_err();
    assert!(error.contains("抽出元Problem"));

    // Problemがない候補をProblemへ関連付けようとするのも拒否する。
    let error = patterns::apply_pattern_proposal(
        &state,
        ApplyPatternProposalPayload {
            problem_id: 0,
            source_reference: String::new(),
            proposal: proposal("画像由来の定石", "image_import"),
            action: "create_new".into(),
            target_pattern_id: None,
            link_relation_type: Some("used".into()),
            parent_pattern_id: None,
        },
    )
    .unwrap_err();
    assert!(error.contains("関連付け"));
}

/// AI編集は、承認するまで定石を変えず、承認したら履歴を残して更新する。
#[test]
fn ai_edit_applies_only_after_approval_and_keeps_history() {
    let (_dir, state) = make_state();
    let id = patterns::create_pattern(&state, "接線による積分評価".into(), "strategy".into()).unwrap();
    patterns::update_pattern(&state, pattern_update(id, 1, "接線による積分評価")).unwrap();
    let before = patterns::get_pattern(&state, id).unwrap();
    let versions_before = patterns::list_pattern_versions(&state, id).unwrap().len();

    // AIへ渡すProposalは、保存済みの内容をそのまま写したもの。
    let source = patterns::pattern_edit_proposal(&state, id).unwrap();
    assert_eq!(source.title, before.title);
    assert_eq!(source.strategies.len(), before.strategies.len());
    assert_eq!(
        patterns::get_pattern(&state, id).unwrap().version,
        before.version,
        "Proposalを作っただけでは定石を変更しない"
    );

    // AIが書き直した想定の候補を承認する。
    let mut edited = source;
    edited.title = "接線を利用した定積分の評価".into();
    edited.strategies.push(PatternProposalStrategy {
        title: "凸性の確認".into(),
        description: "第二次導関数の符号で凸性を確かめる。".into(),
        condition: String::new(),
        reasoning: String::new(),
        sort_order: 2,
    });
    let version = patterns::apply_pattern_edit(&state, id, Some(before.version), edited).unwrap();
    assert_eq!(version, before.version + 1);

    let after = patterns::get_pattern(&state, id).unwrap();
    assert_eq!(after.title, "接線を利用した定積分の評価");
    assert_eq!(after.strategies.len(), before.strategies.len() + 1);
    assert_eq!(
        after.examples, before.examples,
        "AI編集の対象外の項目は保存済みの内容を保つ"
    );
    assert_eq!(after.source_note, before.source_note);
    assert_eq!(
        patterns::list_pattern_versions(&state, id).unwrap().len(),
        versions_before + 1,
        "更新前の内容を履歴へ残す"
    );
}

/// 版が食い違う場合は上書きせず、競合として返す。
#[test]
fn ai_edit_reports_conflict_when_the_pattern_moved_on() {
    let (_dir, state) = make_state();
    let id = patterns::create_pattern(&state, "はさみうちの原理".into(), "strategy".into()).unwrap();
    patterns::update_pattern(&state, pattern_update(id, 1, "はさみうちの原理")).unwrap();
    let stale = patterns::get_pattern(&state, id).unwrap().version;
    // 別の場所で先に更新された状況を作る。
    patterns::update_pattern(&state, pattern_update(id, stale, "はさみうちの原理（更新）")).unwrap();

    let mut edited = patterns::pattern_edit_proposal(&state, id).unwrap();
    edited.title = "AIが書き直したタイトル".into();
    let error = patterns::apply_pattern_edit(&state, id, Some(stale), edited).unwrap_err();
    assert!(error.starts_with("CONFLICT:"), "競合として返す: {error}");
    assert_ne!(
        patterns::get_pattern(&state, id).unwrap().title,
        "AIが書き直したタイトル",
        "競合時は上書きしない"
    );
}

/// AIが上位定石を挙げていても、見つかった既存定石が別物なら特殊化を勧めない。
/// 画像取り込みで、無関係な定石の下へぶら下げる提案が出ていたための回帰テスト。
#[test]
fn child_pattern_is_not_recommended_against_an_unrelated_match() {
    let (_dir, state) = make_state();
    let unrelated = patterns::create_pattern(
        &state,
        "小区間で接線と弦を使い長方形近似の誤差をはさむ".into(),
        "strategy".into(),
    )
    .unwrap();
    let mut update = pattern_update(unrelated, 1, "小区間で接線と弦を使い長方形近似の誤差をはさむ");
    update.summary = "区分求積の誤差を接線と弦ではさんで評価する。".into();
    update.tags = vec!["接線".into()];
    update.facets = PatternFacets {
        domains: vec!["微分法".into()],
        goals: vec!["誤差の評価".into()],
        operations: vec!["はさみうち".into()],
        structures: vec!["長方形近似".into()],
        situations: vec!["区分求積".into()],
    };
    patterns::update_pattern(&state, update).unwrap();

    // 画像から読み取った「平均値の定理の利用」。上位は画像全体の見出しで、ライブラリには無い。
    let mut candidate = proposal("平均値の定理の利用", "image_import");
    candidate.summary = String::new();
    candidate.situation = String::new();
    candidate.principle = String::new();
    candidate.strategies = vec![PatternProposalStrategy {
        title: "平均値の定理を適用する".into(),
        description: "\\(f(b)-f(a)=(b-a)f'(c)\\) と表せる。".into(),
        condition: String::new(),
        reasoning: String::new(),
        sort_order: 1,
    }];
    candidate.tags = vec!["平均値の定理".into()];
    candidate.domains = vec!["微分法".into()];
    candidate.goals = vec!["関数値の差の評価".into()];
    candidate.operations = vec!["平均値の定理".into()];
    candidate.structures = vec!["関数値の差".into()];
    candidate.situations = vec!["区間での評価".into()];
    candidate.search_concepts = vec!["平均値の定理".into(), "関数値の差".into(), "接線".into()];
    candidate.recommended_storage = "child_pattern".into();
    candidate.possible_parent_pattern = Some(kyozai_kobo_lib::models::PatternParentHint {
        title: "関数値の差 \\(f(b)-f(a)\\) の扱い".into(),
        reason: "画像全体の見出し。".into(),
    });

    let mut extraction = PatternExtractionResult {
        schema_version: 1,
        kind: "pattern-extraction".into(),
        patterns: vec![candidate],
    };
    patterns::classify_pattern_proposals(&state, &mut extraction).unwrap();
    assert_ne!(
        extraction.patterns[0].action_recommendation, "create_child_pattern",
        "上位ヒントと一致しない既存定石の特殊化にしない"
    );
}
