//! AI変換プロバイダー抽象化。
//! 将来 OpenAI API 等へ差し替えられるよう、Codex固有処理をここへ隔離する。

use crate::server::rt;
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROGRESS_CHAR_STEP: usize = 250;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

fn text_input(text: &str) -> Value {
    json!({"type": "text", "text": text, "text_elements": []})
}

fn turn_interrupt_params(thread_id: &str, turn_id: &str) -> Value {
    json!({"threadId": thread_id, "turnId": turn_id})
}

fn should_emit_delta_progress(
    received_chars: usize,
    last_reported_chars: usize,
    elapsed: Duration,
) -> bool {
    received_chars.saturating_sub(last_reported_chars) >= PROGRESS_CHAR_STEP
        || elapsed >= PROGRESS_INTERVAL
}

pub struct ConversionRequest {
    /// ジョブの作業ディレクトリ（スレッドのcwdに使う。空のディレクトリ）
    pub work_dir: PathBuf,
    /// 固定指示（変換器としての振る舞い）
    pub developer_instructions: String,
    /// ユーザーターンとして送るテキスト（モード指示＋貼り付けテキスト等）
    pub prompt_text: String,
    /// ローカル画像（前処理済み）
    pub image_paths: Vec<PathBuf>,
    /// 構造化出力のJSON Schema
    pub output_schema: Value,
}

pub trait LatexConversionProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn status(&self, state: &Arc<AppState>) -> Result<Value, String>;
    /// 変換を実行し、モデルの最終メッセージ（生テキスト）を返す。
    /// progress は (状態コード, 表示メッセージ)。cancel が立ったら中断する。
    fn convert(
        &self,
        state: &Arc<AppState>,
        req: &ConversionRequest,
        progress: &dyn Fn(&str, &str),
        cancel: &AtomicBool,
    ) -> Result<String, String>;
}

/// 設定に応じたプロバイダーを返す（現状はCodexのみ）
pub fn provider_for(_state: &Arc<AppState>) -> Arc<dyn LatexConversionProvider> {
    Arc::new(CodexAppServerProvider)
}

pub struct CodexAppServerProvider;

impl LatexConversionProvider for CodexAppServerProvider {
    fn name(&self) -> &'static str {
        "codex-app-server"
    }

    fn status(&self, state: &Arc<AppState>) -> Result<Value, String> {
        super::codex_status(state)
    }

    fn convert(
        &self,
        state: &Arc<AppState>,
        req: &ConversionRequest,
        progress: &dyn Fn(&str, &str),
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        progress("waiting_for_codex", "Codexへ接続しています…");
        super::ensure_running(state)?;

        // 認証確認
        let account = super::request(state, "account/read", json!({}), 20)?;
        let authed = !account.get("account").map(|a| a.is_null()).unwrap_or(true)
            || !account
                .get("requiresOpenaiAuth")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
        if !authed {
            return Err("ChatGPTにログインしていません。設定画面の「Codex / ChatGPT接続」からログインしてください。".into());
        }

        // 通知の購読はターン開始前に行う（取りこぼし防止）
        let mut notif_rx = state
            .codex
            .subscribe_notifs()
            .ok_or("通知チャネルを取得できません")?;

        progress("converting", "スレッドを準備しています…");
        let thread = super::request(
            state,
            "thread/start",
            json!({
                "ephemeral": true,
                "cwd": req.work_dir.to_string_lossy(),
                "sandbox": "read-only",
                "approvalPolicy": "never",
                "developerInstructions": req.developer_instructions,
            }),
            60,
        )?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(|v| v.as_str())
            .ok_or("スレッドIDを取得できません")?
            .to_string();

        // 入力の組み立て（テキスト＋ローカル画像）
        let mut input: Vec<Value> = vec![text_input(&req.prompt_text)];
        for p in &req.image_paths {
            input.push(json!({"type": "localImage", "path": p.to_string_lossy()}));
        }

        progress("converting", "Codexで変換しています…");
        let turn = super::request(
            state,
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": input,
                "outputSchema": req.output_schema,
            }),
            120,
        )?;
        let turn_id = turn
            .pointer("/turn/id")
            .and_then(|v| v.as_str())
            .ok_or("ターンIDを取得できません")?
            .to_string();

        // 通知を待つ（全体タイムアウト10分）
        let deadline = Instant::now() + Duration::from_secs(600);
        let mut last_message = String::new();
        let mut delta_len: usize = 0;
        let mut last_progress_len: usize = 0;
        let mut last_progress_at = Instant::now();

        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = super::request(
                    state,
                    "turn/interrupt",
                    turn_interrupt_params(&thread_id, &turn_id),
                    10,
                );
                return Err("キャンセルされました".into());
            }
            if Instant::now() > deadline {
                let _ = super::request(
                    state,
                    "turn/interrupt",
                    turn_interrupt_params(&thread_id, &turn_id),
                    10,
                );
                return Err("変換がタイムアウトしました（10分）".into());
            }
            let recv = rt().block_on(async {
                tokio::time::timeout(Duration::from_secs(2), notif_rx.recv()).await
            });
            let v = match recv {
                Ok(Ok(v)) => v,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => return Err("Codexとの接続が切断されました".into()),
                Err(_) => continue, // タイムアウト → キャンセル確認へ
            };
            let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = v.get("params").cloned().unwrap_or(Value::Null);
            let tid = params.get("threadId").and_then(|t| t.as_str()).unwrap_or("");
            if !tid.is_empty() && tid != thread_id {
                continue;
            }
            let received_turn_id = params
                .get("turnId")
                .or_else(|| params.pointer("/turn/id"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if !received_turn_id.is_empty() && received_turn_id != turn_id {
                continue;
            }
            match method {
                "item/agentMessage/delta" => {
                    if let Some(d) = params.get("delta").and_then(|d| d.as_str()) {
                        delta_len += d.chars().count();
                        let now = Instant::now();
                        if should_emit_delta_progress(
                            delta_len,
                            last_progress_len,
                            now.duration_since(last_progress_at),
                        ) {
                            progress(
                                "converting",
                                &format!("Codexで変換しています…（{}文字受信）", delta_len),
                            );
                            last_progress_len = delta_len;
                            last_progress_at = now;
                        }
                    }
                }
                "item/completed" => {
                    if params.pointer("/item/type").and_then(|t| t.as_str()) == Some("agentMessage") {
                        if let Some(text) = params.pointer("/item/text").and_then(|t| t.as_str()) {
                            last_message = text.to_string();
                        }
                    }
                }
                "turn/completed" => {
                    let status = params
                        .pointer("/turn/status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("");
                    if status == "failed" || status == "interrupted" {
                        let msg = params
                            .pointer("/turn/error/message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("変換に失敗しました");
                        return Err(format!("Codexターンが失敗しました: {}", msg));
                    }
                    if delta_len > 0 {
                        progress(
                            "converting",
                            &format!("Codexからの受信が完了しました（{}文字）", delta_len),
                        );
                    }
                    // itemsから最終メッセージを取得（deltaより確実）
                    if let Some(items) = params.pointer("/turn/items").and_then(|i| i.as_array()) {
                        for item in items {
                            if item.get("type").and_then(|t| t.as_str()) == Some("agentMessage") {
                                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                    last_message = text.to_string();
                                }
                            }
                        }
                    }
                    if last_message.is_empty() {
                        return Err("Codexから出力を受け取れませんでした".into());
                    }
                    return Ok(last_message);
                }
                "error" => {
                    let will_retry = params
                        .get("willRetry")
                        .and_then(|w| w.as_bool())
                        .unwrap_or(false);
                    if !will_retry {
                        let msg = params
                            .pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Codexでエラーが発生しました");
                        let code = params
                            .pointer("/error/codexErrorInfo")
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        if code.contains("usageLimitExceeded") {
                            return Err("ChatGPTの利用制限に達しました。時間をおいて再試行してください。".into());
                        }
                        return Err(format!("Codexエラー: {}", msg));
                    }
                    progress("converting", "一時的なエラーが発生し、Codexが再試行しています…");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_includes_required_text_elements() {
        assert_eq!(
            text_input("問題文"),
            json!({"type": "text", "text": "問題文", "text_elements": []})
        );
    }

    #[test]
    fn interrupt_params_include_turn_id() {
        assert_eq!(
            turn_interrupt_params("thread-1", "turn-2"),
            json!({"threadId": "thread-1", "turnId": "turn-2"})
        );
    }

    #[test]
    fn delta_progress_is_throttled_by_chars_or_time() {
        assert!(!should_emit_delta_progress(
            249,
            0,
            Duration::from_millis(499)
        ));
        assert!(should_emit_delta_progress(
            250,
            0,
            Duration::from_millis(1)
        ));
        assert!(should_emit_delta_progress(
            1,
            0,
            Duration::from_millis(500)
        ));
    }
}
