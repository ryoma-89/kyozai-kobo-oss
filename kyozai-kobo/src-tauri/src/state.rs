use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::broadcast;

/// データ変更等をUI（デスクトップ/Web両方）へ通知するイベント
#[derive(Clone, Serialize, Debug)]
pub struct AppEvent {
    /// "problems" | "tree" | "projects" | "parts" | "templates" | "settings"
    /// | "compile" | "ai_job" | "codex" | "server"
    pub kind: String,
    /// 発生元コマンド名など
    pub cmd: String,
    /// 関連ID（problemId / projectId / partId / templateId / itemId / jobId 等）
    pub ids: serde_json::Value,
}

pub struct AppState {
    pub conn: Mutex<Connection>,
    pub data_dir: PathBuf,
    /// ドキュメントフォルダ（PDF出力先の既定）。起動時に解決
    pub documents_dir: Option<PathBuf>,
    /// Tauriリソースフォルダ（グラフアプリ検出用）。起動時に解決
    pub resource_dir: Option<PathBuf>,
    /// 変更イベントのブロードキャスト
    pub events: broadcast::Sender<AppEvent>,
    /// 教材サーバー（HTTP）の制御
    pub server: crate::server::ServerControl,
    /// Codex App Server（子プロセス）の管理
    pub codex: crate::codex::CodexManager,
    /// AI変換ジョブのキュー
    pub ai: crate::ai::AiRunner,
}

impl AppState {
    pub fn new(conn: Connection, data_dir: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(256);
        AppState {
            conn: Mutex::new(conn),
            data_dir,
            documents_dir: None,
            resource_dir: None,
            events: tx,
            server: Default::default(),
            codex: Default::default(),
            ai: Default::default(),
        }
    }

    pub fn emit(&self, kind: &str, cmd: &str, ids: serde_json::Value) {
        let _ = self.events.send(AppEvent {
            kind: kind.to_string(),
            cmd: cmd.to_string(),
            ids,
        });
    }

    pub fn attachments_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("attachments");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    pub fn graph_assets_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("graph_assets");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// graph.json と派生出力をグラフ単位で保持する正本フォルダ。
    /// 既存の外部アプリ連携と同じ graph_assets 配下を使い、バックアップ経路を共用する。
    pub fn graph_dir(&self, graph_id: &str) -> PathBuf {
        self.graph_assets_dir().join(graph_id)
    }

    pub fn part_attachments_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("part_attachments");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// Web/AI変換用アップロードの一時保存先
    pub fn uploads_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("uploads");
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    /// AI変換ジョブの成果物（プレビューPDF等）保存先
    pub fn ai_jobs_dir(&self) -> PathBuf {
        let dir = self.data_dir.join("ai_jobs");
        std::fs::create_dir_all(&dir).ok();
        dir
    }
}

pub fn err_str<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
