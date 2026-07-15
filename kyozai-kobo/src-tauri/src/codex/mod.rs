//! Codex App Server 統合。
//! PC上の `codex app-server` を子プロセスとして起動し、標準入出力の
//! JSONL (JSON-RPC 2.0) で通信する。ブラウザへは絶対に直接公開しない。
//! 認証（ChatGPTデバイスコード等）はCodex側の公式フローに委譲し、
//! トークン類はこのアプリでは保存・転送しない。

pub mod provider;

use crate::server::rt;
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, oneshot};

/// デバイスコード認証の進行状態
#[derive(Clone, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    pub login_id: String,
    pub method: String,
    pub user_code: Option<String>,
    pub verification_url: Option<String>,
    pub auth_url: Option<String>,
    /// pending | success | failed
    pub status: String,
    pub error: Option<String>,
}

struct ProcHandle {
    stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
    alive: Arc<AtomicBool>,
    version: String,
    exe_display: String,
    child_pid: Option<u32>,
}

#[derive(Default)]
pub struct CodexManager {
    proc: Mutex<Option<ProcHandle>>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    next_id: AtomicI64,
    /// 生の通知 {"method":..,"params":..} のブロードキャスト
    notifs: Mutex<Option<broadcast::Sender<Value>>>,
    pub login: Mutex<Option<LoginState>>,
    pub last_error: Mutex<Option<String>>,
    pub log: Mutex<VecDeque<String>>,
}

impl CodexManager {
    pub fn log_line(&self, line: &str) {
        if let Ok(mut log) = self.log.lock() {
            log.push_back(format!("[{}] {}", crate::db::now_str(), line));
            while log.len() > 200 {
                log.pop_front();
            }
        }
    }

    pub fn subscribe_notifs(&self) -> Option<broadcast::Receiver<Value>> {
        self.notifs.lock().ok()?.as_ref().map(|tx| tx.subscribe())
    }

    fn is_running(&self) -> bool {
        self.proc
            .lock()
            .ok()
            .and_then(|p| p.as_ref().map(|h| h.alive.load(Ordering::SeqCst)))
            .unwrap_or(false)
    }
}

// ---- 実行ファイルの検出 ----

fn get_setting(state: &AppState, key: &str) -> Option<String> {
    let conn = state.conn.lock().ok()?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key=?1",
        rusqlite::params![key],
        |r| r.get(0),
    )
    .ok()
    .filter(|v: &String| !v.trim().is_empty())
}

/// npmラッパー(codex/codex.cmd)から実体のcodex.exeを探す
fn resolve_npm_vendor_exe(shim: &std::path::Path) -> Option<PathBuf> {
    let prefix = shim.parent()?;
    let candidates = [
        prefix.join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe"),
        prefix.join("node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/codex/codex.exe"),
        prefix.join("node_modules/@openai/codex/bin/codex.exe"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// 設定値やPATH上のshimを、実際に直接起動できるCodexバイナリへ解決する。
/// Microsoft Store版のWindowsApps内バイナリは存在していてもACLにより
/// 子プロセス起動できない場合があるため、`--version` 成功まで確認する。
fn usable_codex_candidate(path: &std::path::Path) -> Option<PathBuf> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "cmd" || extension == "ps1" || extension.is_empty() {
        if let Some(vendor) = resolve_npm_vendor_exe(path) {
            if codex_version(&vendor).is_some() {
                return Some(vendor);
            }
        }
    }
    if extension == "exe" && codex_version(path).is_some() {
        return Some(path.to_path_buf());
    }
    None
}

/// Codex実行ファイルを検出する（設定 → PATH → npmグローバル実体）
pub fn detect_codex_exe(state: &AppState) -> Option<PathBuf> {
    if let Some(p) = get_setting(state, "codex_path") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            if let Some(usable) = usable_codex_candidate(&pb) {
                return Some(usable);
            }
        }
    }

    // where.exe はWindowsAppsの実行不能なcodex.exeだけを返す環境がある。
    // まずPATHを直接走査し、npm shimからvendor実体を全ディレクトリ横断で探す。
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    for dir in &path_dirs {
        for name in ["codex.cmd", "codex.ps1", "codex"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                if let Some(usable) = usable_codex_candidate(&candidate) {
                    return Some(usable);
                }
            }
        }
    }
    for dir in &path_dirs {
        let candidate = dir.join("codex.exe");
        if candidate.exists() {
            if let Some(usable) = usable_codex_candidate(&candidate) {
                return Some(usable);
            }
        }
    }

    // PATHの表記揺れに備える最後のフォールバック。
    let out = {
        let mut cmd = std::process::Command::new("where.exe");
        cmd.arg("codex");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.output().ok()?
    };
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut shim: Option<PathBuf> = None;
    for line in text.lines() {
        let p = PathBuf::from(line.trim());
        if !p.exists() {
            continue;
        }
        if let Some(usable) = usable_codex_candidate(&p) {
            return Some(usable);
        }
        shim.get_or_insert(p);
    }
    let shim = shim?;
    usable_codex_candidate(&shim)
}

pub fn codex_version(exe: &std::path::Path) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

// ---- プロセス管理 ----

struct ReaderCtx {
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    notif_tx: broadcast::Sender<Value>,
    alive: Arc<AtomicBool>,
    state: std::sync::Weak<AppState>,
    stdin: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
}

async fn reader_loop(stdout: tokio::process::ChildStdout, ctx: ReaderCtx) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let has_id = v.get("id").map(|i| !i.is_null()).unwrap_or(false);
        let has_method = v.get("method").and_then(|m| m.as_str()).is_some();

        if has_method && !has_id {
            // 通知
            handle_notification(&ctx, &v);
            let _ = ctx.notif_tx.send(v);
        } else if has_method && has_id {
            // サーバー→クライアント要求（承認等）。転記用途では発生しない想定のため
            // 常にエラー応答してハングを防ぐ
            let id = v.get("id").cloned().unwrap_or(Value::Null);
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "この操作はサポートされていません"}
            });
            let mut stdin = ctx.stdin.lock().await;
            let _ = stdin
                .write_all(format!("{}\n", resp).as_bytes())
                .await;
        } else if has_id {
            // 応答
            let id = v.get("id").and_then(|i| i.as_i64());
            if let Some(id) = id {
                let tx = ctx.pending.lock().ok().and_then(|mut p| p.remove(&id));
                if let Some(tx) = tx {
                    let _ = tx.send(v);
                }
            }
        }
    }
    // EOF: プロセス終了
    ctx.alive.store(false, Ordering::SeqCst);
    if let Some(state) = ctx.state.upgrade() {
        state.codex.log_line("codex app-server が終了しました");
        state.emit("codex", "codex_exited", Value::Null);
    }
}

fn handle_notification(ctx: &ReaderCtx, v: &Value) {
    let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    let Some(state) = ctx.state.upgrade() else { return };
    match method {
        "account/login/completed" => {
            let success = params.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
            let error = params
                .get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string());
            if let Ok(mut login) = state.codex.login.lock() {
                if let Some(l) = login.as_mut() {
                    l.status = if success { "success".into() } else { "failed".into() };
                    l.error = error;
                }
            }
            state.codex.log_line(if success {
                "ChatGPTログインが完了しました"
            } else {
                "ChatGPTログインに失敗しました"
            });
            state.emit("codex", "login_completed", json!({"success": success}));
        }
        "account/updated" | "account/rateLimits/updated" => {
            state.emit("codex", method, Value::Null);
        }
        _ => {}
    }
}

/// app-server を起動して initialize まで行う（既に起動済みなら何もしない）
pub fn ensure_running(state: &Arc<AppState>) -> Result<(), String> {
    if state.codex.is_running() {
        return Ok(());
    }
    let exe = detect_codex_exe(state).ok_or_else(|| {
        "Codexが見つかりません。`npm install -g @openai/codex` でインストールするか、設定画面でパスを指定してください。".to_string()
    })?;
    let version = codex_version(&exe).unwrap_or_else(|| "不明".into());

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = rt()
        .block_on(async { cmd.spawn() })
        .map_err(|e| format!("codex app-server の起動に失敗しました: {}", e))?;

    let stdin = child.stdin.take().ok_or("stdinを取得できません")?;
    let stdout = child.stdout.take().ok_or("stdoutを取得できません")?;
    let stderr = child.stderr.take();
    let pid = child.id();

    let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
    let alive = Arc::new(AtomicBool::new(true));
    let (notif_tx, _) = broadcast::channel::<Value>(512);

    // 読み取りタスク
    let ctx = ReaderCtx {
        pending: state.codex.pending.clone(),
        notif_tx: notif_tx.clone(),
        alive: alive.clone(),
        state: Arc::downgrade(state),
        stdin: stdin.clone(),
    };
    rt().spawn(reader_loop(stdout, ctx));

    // stderr → ログ
    if let Some(stderr) = stderr {
        let state_weak = Arc::downgrade(state);
        rt().spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(st) = state_weak.upgrade() {
                    st.codex.log_line(&format!("stderr: {}", line));
                }
            }
        });
    }

    // 子プロセスの終了待ち（ゾンビ回収）
    rt().spawn(async move {
        let _ = child.wait().await;
    });

    {
        let mut proc = state.codex.proc.lock().map_err(|e| e.to_string())?;
        *proc = Some(ProcHandle {
            stdin,
            alive,
            version: version.clone(),
            exe_display: exe.to_string_lossy().to_string(),
            child_pid: pid,
        });
    }
    {
        let mut notifs = state.codex.notifs.lock().map_err(|e| e.to_string())?;
        *notifs = Some(notif_tx);
    }

    state.codex.log_line(&format!("codex app-server を起動しました ({})", version));

    // initialize の応答後に initialized 通知を送って初期化を完了する。
    // 途中で失敗したプロセスを残すと次回の再接続が壊れるため、必ず片付ける。
    let initialized = (|| -> Result<(), String> {
        request(
            state,
            "initialize",
            initialize_params(),
            20,
        )?;
        send_notification(state, "initialized")
    })();
    if let Err(error) = initialized {
        if let Ok(mut last_error) = state.codex.last_error.lock() {
            *last_error = Some(error.clone());
        }
        state
            .codex
            .log_line(&format!("codex app-server の初期化に失敗しました: {}", error));
        shutdown(state);
        return Err(error);
    }
    state.emit("codex", "codex_started", Value::Null);
    Ok(())
}

/// アプリ終了時などの停止
pub fn shutdown(state: &Arc<AppState>) {
    let handle = state.codex.proc.lock().ok().and_then(|mut p| p.take());
    if let Ok(mut notifs) = state.codex.notifs.lock() {
        *notifs = None;
    }
    if let Ok(mut pending) = state.codex.pending.lock() {
        pending.clear();
    }
    if let Some(h) = handle {
        h.alive.store(false, Ordering::SeqCst);
        if let Some(pid) = h.child_pid {
            #[cfg(windows)]
            {
                let mut cmd = std::process::Command::new("taskkill");
                cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
                let _ = cmd.output();
            }
            #[cfg(not(windows))]
            {
                let _ = pid;
            }
        }
        state.codex.log_line("codex app-server を停止しました");
    }
}

fn rpc_request_message(id: i64, method: &str, params: Option<Value>) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    if let Some(params) = params {
        if let Some(object) = message.as_object_mut() {
            object.insert("params".to_string(), params);
        }
    }
    message
}

fn initialize_params() -> Value {
    json!({
        "clientInfo": {
            "name": "kyozai-kobo",
            "title": "教材工房",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": Value::Null,
    })
}

fn rpc_notification_message(method: &str) -> Value {
    // ClientNotification の wire schema は method のみ（initialized に params はない）。
    json!({"method": method})
}

fn send_notification(state: &Arc<AppState>, method: &str) -> Result<(), String> {
    let (stdin, alive) = {
        let proc = state.codex.proc.lock().map_err(|e| e.to_string())?;
        let h = proc.as_ref().ok_or("codex app-server が起動していません")?;
        (h.stdin.clone(), h.alive.clone())
    };
    if !alive.load(Ordering::SeqCst) {
        return Err("codex app-server が停止しています。再起動してください".into());
    }

    let line = format!("{}\n", rpc_notification_message(method));
    rt().block_on(async move {
        let mut stdin = stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("送信に失敗しました: {}", e))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("送信に失敗しました: {}", e))
    })
}

/// JSON-RPCリクエストを送り、応答の result を返す
pub fn request(
    state: &Arc<AppState>,
    method: &str,
    params: Value,
    timeout_secs: u64,
) -> Result<Value, String> {
    request_inner(state, method, Some(params), timeout_secs)
}

/// params を持たない unit request を送る。
fn request_without_params(
    state: &Arc<AppState>,
    method: &str,
    timeout_secs: u64,
) -> Result<Value, String> {
    request_inner(state, method, None, timeout_secs)
}

fn request_inner(
    state: &Arc<AppState>,
    method: &str,
    params: Option<Value>,
    timeout_secs: u64,
) -> Result<Value, String> {
    let (stdin, alive) = {
        let proc = state.codex.proc.lock().map_err(|e| e.to_string())?;
        let h = proc.as_ref().ok_or("codex app-server が起動していません")?;
        (h.stdin.clone(), h.alive.clone())
    };
    if !alive.load(Ordering::SeqCst) {
        return Err("codex app-server が停止しています。再起動してください".into());
    }
    let id = state.codex.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = oneshot::channel::<Value>();
    state
        .codex
        .pending
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id, tx);

    let msg = rpc_request_message(id, method, params);
    let line = format!("{}\n", msg);

    let result = rt().block_on(async move {
        {
            let mut stdin = stdin.lock().await;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| format!("送信に失敗しました: {}", e))?;
            stdin.flush().await.map_err(|e| format!("送信に失敗しました: {}", e))?;
        }
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err("応答チャネルが閉じられました".to_string()),
            Err(_) => Err(format!("{} がタイムアウトしました（{}秒）", method, timeout_secs)),
        }
    });

    // タイムアウト時はpendingから除去
    if result.is_err() {
        if let Ok(mut p) = state.codex.pending.lock() {
            p.remove(&id);
        }
    }
    let v = result?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("不明なエラー");
        if let Ok(mut le) = state.codex.last_error.lock() {
            *le = Some(msg.to_string());
        }
        return Err(format!("Codexエラー: {}", msg));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

// ---- 高水準API（dispatchから呼ばれる） ----

/// 接続・アカウント状態のまとめ
pub fn codex_status(state: &Arc<AppState>) -> Result<Value, String> {
    let exe = detect_codex_exe(state);
    let installed = exe.is_some();
    let (running, version, exe_display) = {
        let proc = state.codex.proc.lock().map_err(|e| e.to_string())?;
        match proc.as_ref() {
            Some(h) if h.alive.load(Ordering::SeqCst) => {
                (true, h.version.clone(), h.exe_display.clone())
            }
            _ => (
                false,
                exe.as_ref()
                    .and_then(|p| codex_version(p))
                    .unwrap_or_default(),
                exe.as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        }
    };
    let mut account = Value::Null;
    let mut rate_limits = Value::Null;
    if running {
        if let Ok(a) = request(state, "account/read", json!({}), 15) {
            account = a;
        }
        if let Ok(r) = request_without_params(state, "account/rateLimits/read", 15) {
            rate_limits = r;
        }
    }
    let login = state.codex.login.lock().map_err(|e| e.to_string())?.clone();
    let last_error = state.codex.last_error.lock().map_err(|e| e.to_string())?.clone();
    let log: Vec<String> = state
        .codex
        .log
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .cloned()
        .collect();
    Ok(json!({
        "installed": installed,
        "exePath": exe_display,
        "version": version,
        "running": running,
        "account": account,
        "rateLimits": rate_limits,
        "login": login,
        "lastError": last_error,
        "log": log,
    }))
}

/// ChatGPTログイン開始。method: "deviceCode"（推奨） | "browser"
pub fn login_start(state: &Arc<AppState>, method: &str) -> Result<Value, String> {
    ensure_running(state)?;
    let params = if method == "browser" {
        json!({"type": "chatgpt"})
    } else {
        json!({"type": "chatgptDeviceCode"})
    };
    let result = request(state, "account/login/start", params, 60)?;
    let login = LoginState {
        login_id: result
            .get("loginId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        method: method.to_string(),
        user_code: result
            .get("userCode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        verification_url: result
            .get("verificationUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        auth_url: result
            .get("authUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        status: "pending".into(),
        error: None,
    };
    {
        let mut l = state.codex.login.lock().map_err(|e| e.to_string())?;
        *l = Some(login.clone());
    }
    state.codex.log_line("ChatGPTログインを開始しました");
    state.emit("codex", "login_started", Value::Null);
    serde_json::to_value(login).map_err(|e| e.to_string())
}

pub fn login_cancel(state: &Arc<AppState>) -> Result<(), String> {
    let login_id = {
        let l = state.codex.login.lock().map_err(|e| e.to_string())?;
        l.as_ref().map(|x| x.login_id.clone())
    };
    if let Some(id) = login_id {
        let _ = request(state, "account/login/cancel", json!({"loginId": id}), 15);
    }
    let mut l = state.codex.login.lock().map_err(|e| e.to_string())?;
    *l = None;
    state.emit("codex", "login_cancelled", Value::Null);
    Ok(())
}

pub fn logout(state: &Arc<AppState>) -> Result<(), String> {
    ensure_running(state)?;
    request_without_params(state, "account/logout", 20)?;
    state.codex.log_line("ChatGPTからログアウトしました");
    state.emit("codex", "logout", Value::Null);
    Ok(())
}

/// 接続テスト: 起動＋initialize＋account/read
pub fn test_connection(state: &Arc<AppState>) -> Result<Value, String> {
    ensure_running(state)?;
    let account = request(state, "account/read", json!({}), 20)?;
    Ok(json!({"ok": true, "account": account}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_request_omits_params() {
        let message = rpc_request_message(7, "account/logout", None);

        assert_eq!(message.get("method"), Some(&json!("account/logout")));
        assert!(message.get("params").is_none());
    }

    #[test]
    fn parameterized_request_keeps_params() {
        let message = rpc_request_message(8, "account/read", Some(json!({})));

        assert_eq!(message.get("params"), Some(&json!({})));
    }

    #[test]
    fn initialized_notification_has_no_params_or_id() {
        let message = rpc_notification_message("initialized");

        assert_eq!(message, json!({"method": "initialized"}));
    }

    #[test]
    fn initialize_explicitly_sends_null_capabilities() {
        assert_eq!(initialize_params().get("capabilities"), Some(&Value::Null));
    }

    #[test]
    fn npm_shim_resolves_to_windows_vendor_binary() {
        let dir = tempdir::TempDir::new("codex-shim-test").unwrap();
        let shim = dir.path().join("codex.cmd");
        std::fs::write(&shim, b"@echo off").unwrap();
        let vendor = dir
            .path()
            .join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe");
        std::fs::create_dir_all(vendor.parent().unwrap()).unwrap();
        std::fs::write(&vendor, b"test").unwrap();

        assert_eq!(resolve_npm_vendor_exe(&shim), Some(vendor));
    }
}
