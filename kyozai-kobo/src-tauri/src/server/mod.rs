//! PC教材サーバー: 同一プロセス内のAxum HTTPサーバー。
//! 標準では 127.0.0.1 のみで待ち受け、外部からは Tailscale Serve 経由で利用する。
//! ブラウザ版UIは埋め込みの dist（デスクトップと同じReactアプリ）を配信する。

pub mod auth;
pub mod backup;
pub mod system;

use crate::commands::dispatch::{dispatch, Origin};
use crate::commands::graphs;
use crate::db::now_str;
use crate::state::AppState;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{sse, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use include_dir::{include_dir, Dir};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_stream::StreamExt;

static DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../dist");

pub const DEFAULT_PORT: u16 = 8760;

/// アップロード上限（マルチパート全体）
const MAX_UPLOAD_BYTES: usize = 30 * 1024 * 1024;

pub struct RunningServer {
    pub port: u16,
    pub lan: bool,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Default)]
pub struct ServerControl {
    pub running: Mutex<Option<RunningServer>>,
    pub pairing_code: Mutex<String>,
    /// (連続失敗回数, ロック解除時刻)
    pub pair_fails: Mutex<(u32, Option<std::time::Instant>)>,
    pub log: Mutex<VecDeque<String>>,
}

impl ServerControl {
    pub fn log_line(&self, line: &str) {
        if let Ok(mut log) = self.log.lock() {
            log.push_back(format!("[{}] {}", now_str(), line));
            while log.len() > 300 {
                log.pop_front();
            }
        }
    }
}

/// サーバー専用のtokioランタイム（Tauriのランタイムやテストから独立）
pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokioランタイムの作成に失敗")
    })
}

// ---- サーバー設定（server_settingsテーブル） ----

pub fn get_server_setting(state: &AppState, key: &str) -> Option<String> {
    let conn = state.conn.lock().ok()?;
    conn.query_row(
        "SELECT value FROM server_settings WHERE key=?1",
        params![key],
        |r| r.get(0),
    )
    .ok()
}

pub fn set_server_setting(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO server_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn configured_port(state: &AppState) -> u16 {
    get_server_setting(state, "port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn lan_mode(state: &AppState) -> bool {
    get_server_setting(state, "lan_mode").as_deref() == Some("1")
}

// ---- 起動・停止・状態 ----

pub fn start(state: &Arc<AppState>) -> Result<Value, String> {
    {
        let running = state.server.running.lock().map_err(|e| e.to_string())?;
        if running.is_some() {
            return Err("サーバーは既に起動しています".into());
        }
    }
    let port = configured_port(state);
    let lan = lan_mode(state);
    let bind_addr = if lan {
        format!("0.0.0.0:{}", port)
    } else {
        format!("127.0.0.1:{}", port)
    };

    let listener = rt()
        .block_on(tokio::net::TcpListener::bind(&bind_addr))
        .map_err(|e| format!("ポート {} で待ち受けできません: {}", port, e))?;

    // ペアリングコードを新しく発行
    {
        let mut code = state.server.pairing_code.lock().map_err(|e| e.to_string())?;
        *code = auth::generate_pairing_code();
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let app = build_router(state.clone());
    rt().spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });

    {
        let mut running = state.server.running.lock().map_err(|e| e.to_string())?;
        *running = Some(RunningServer {
            port,
            lan,
            shutdown: Some(tx),
        });
    }
    state.server.log_line(&format!(
        "サーバーを起動しました: http://127.0.0.1:{}{}",
        port,
        if lan { "（LAN公開モード）" } else { "" }
    ));
    state.emit("server", "server_start", json!({}));
    status(state)
}

pub fn stop(state: &Arc<AppState>) -> Result<Value, String> {
    {
        let mut running = state.server.running.lock().map_err(|e| e.to_string())?;
        match running.take() {
            Some(mut rs) => {
                if let Some(tx) = rs.shutdown.take() {
                    let _ = tx.send(());
                }
            }
            None => return Err("サーバーは起動していません".into()),
        }
    }
    state.server.log_line("サーバーを停止しました");
    state.emit("server", "server_stop", json!({}));
    status(state)
}

pub fn regen_pairing(state: &Arc<AppState>) -> Result<Value, String> {
    {
        let mut code = state.server.pairing_code.lock().map_err(|e| e.to_string())?;
        *code = auth::generate_pairing_code();
    }
    state.server.log_line("ペアリングコードを再発行しました");
    status(state)
}

/// サーバー状態（デスクトップ管理画面用。ペアリングコードを含む）
pub fn status(state: &Arc<AppState>) -> Result<Value, String> {
    let (running, port, lan) = {
        let running = state.server.running.lock().map_err(|e| e.to_string())?;
        match running.as_ref() {
            Some(r) => (true, r.port, r.lan),
            None => (false, configured_port(state), lan_mode(state)),
        }
    };
    let code = state
        .server
        .pairing_code
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let log: Vec<String> = state
        .server
        .log
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .cloned()
        .collect();
    let devices = list_devices(state)?;
    Ok(json!({
        "running": running,
        "port": port,
        "lanMode": lan,
        "localUrl": format!("http://127.0.0.1:{}", port),
        "pairingCode": if running { Value::String(code) } else { Value::Null },
        "activeSessions": auth::active_session_count(state),
        "devices": devices,
        "log": log,
    }))
}

pub fn list_devices(state: &Arc<AppState>) -> Result<Value, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, device_name, user_agent, created_at, last_seen_at, revoked
             FROM trusted_devices ORDER BY last_seen_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<Value> = stmt
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "deviceName": r.get::<_, String>(1)?,
                "userAgent": r.get::<_, String>(2)?,
                "createdAt": r.get::<_, String>(3)?,
                "lastSeenAt": r.get::<_, String>(4)?,
                "revoked": r.get::<_, i64>(5)? != 0,
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Value::Array(rows))
}

pub fn revoke_device(state: &Arc<AppState>, device_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE trusted_devices SET revoked=1 WHERE id=?1",
        params![device_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM web_sessions WHERE device_id=?1",
        params![device_id],
    )
    .map_err(|e| e.to_string())?;
    state.server.log_line(&format!("端末 {} のアクセスを取り消しました", device_id));
    Ok(())
}

// ---- ルーター ----

type Shared = Arc<AppState>;

pub fn build_router(state: Shared) -> Router {
    let protected = Router::new()
        .route("/invoke/{cmd}", post(invoke_handler))
        .route("/events", get(sse_handler))
        .route("/auth/logout", post(auth_logout))
        .route("/files/attachment/{name}", get(file_attachment))
        .route("/files/part-attachment/{name}", get(file_part_attachment))
        .route("/files/template-asset/{tid}/{name}", get(file_template_asset))
        .route("/files/ai-job/{uuid}/{name}", get(file_ai_job))
        .route("/files/build", get(file_build))
        .route("/graphs/{id}/files/{format}", get(file_graph))
        .route("/uploads/attachment", post(upload_attachment))
        .route("/uploads/part-attachment", post(upload_part_attachment))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/pair", post(auth_pair))
        .route("/api/auth/me", get(auth_me))
        .nest("/api", protected)
        .fallback(static_handler)
        .with_state(state)
}

// ---- 認証まわり ----

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookies.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix(&format!("{}=", auth::SESSION_COOKIE)) {
            return Some(v.to_string());
        }
    }
    None
}

fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

/// Originヘッダーが付いている場合、Host（またはX-Forwarded-Host）と一致することを要求
fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true; // Origin無し（同一オリジンGET等）は許可
    };
    if origin == "null" {
        return false;
    }
    let origin_host = origin
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let fwd_host = headers.get("x-forwarded-host").and_then(|v| v.to_str().ok());
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    [fwd_host, host]
        .into_iter()
        .flatten()
        .any(|h| h.eq_ignore_ascii_case(origin_host))
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

async fn require_auth(
    State(state): State<Shared>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    // CSRF対策: 変更系メソッドはカスタムヘッダー必須 + Origin検証
    let m = req.method();
    if m != Method::GET && m != Method::HEAD {
        let has_header = headers
            .get("x-requested-with")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "kyozai-kobo")
            .unwrap_or(false);
        if !has_header || !origin_ok(headers) {
            return err_json(StatusCode::FORBIDDEN, "不正なリクエストです（CSRF検証）");
        }
    }
    let Some(token) = cookie_token(headers) else {
        return err_json(StatusCode::UNAUTHORIZED, "未認証です。ペアリングしてください");
    };
    let state2 = state.clone();
    let valid =
        tokio::task::spawn_blocking(move || auth::validate_session(&state2, &token)).await;
    match valid {
        Ok(Some(_device_id)) => next.run(req).await,
        _ => err_json(StatusCode::UNAUTHORIZED, "セッションが無効です。再ペアリングしてください"),
    }
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "app": "kyozai-kobo", "version": env!("CARGO_PKG_VERSION") }))
}

#[derive(serde::Deserialize)]
struct PairBody {
    code: String,
    #[serde(default)]
    device_name: Option<String>,
    #[serde(default)]
    #[serde(rename = "deviceName")]
    device_name_camel: Option<String>,
}

async fn auth_pair(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<PairBody>,
) -> Response {
    if !origin_ok(&headers) {
        return err_json(StatusCode::FORBIDDEN, "不正なリクエストです");
    }
    // レート制限: 5回失敗で60秒ロック
    {
        let mut fails = match state.server.pair_fails.lock() {
            Ok(f) => f,
            Err(_) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, "内部エラー"),
        };
        if let (n, Some(until)) = &*fails {
            if *n >= 5 {
                if until.elapsed() < std::time::Duration::from_secs(60) {
                    return err_json(
                        StatusCode::TOO_MANY_REQUESTS,
                        "試行回数が多すぎます。しばらく待ってから再試行してください",
                    );
                }
                *fails = (0, None);
            }
        }
    }

    let expected = state
        .server
        .pairing_code
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default();
    let input = body.code.trim().to_string();
    if expected.is_empty() || input != expected {
        if let Ok(mut fails) = state.server.pair_fails.lock() {
            fails.0 += 1;
            fails.1 = Some(std::time::Instant::now());
        }
        state.server.log_line("ペアリング失敗（コード不一致）");
        return err_json(StatusCode::UNAUTHORIZED, "ペアリングコードが違います");
    }

    let device_name = body
        .device_name_camel
        .or(body.device_name)
        .unwrap_or_default();
    let device_name = if device_name.trim().is_empty() {
        "名称未設定の端末".to_string()
    } else {
        device_name.trim().chars().take(60).collect()
    };
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();

    let state2 = state.clone();
    let dn = device_name.clone();
    let token = match tokio::task::spawn_blocking(move || auth::create_session(&state2, &dn, &ua)).await {
        Ok(Ok(t)) => t,
        _ => return err_json(StatusCode::INTERNAL_SERVER_ERROR, "セッションの作成に失敗しました"),
    };

    // 成功: コードは1回限り → 再発行
    if let Ok(mut code) = state.server.pairing_code.lock() {
        *code = auth::generate_pairing_code();
    }
    if let Ok(mut fails) = state.server.pair_fails.lock() {
        *fails = (0, None);
    }
    state.server.log_line(&format!("ペアリング成功: {}", device_name));
    state.emit("server", "device_paired", json!({}));

    let secure = if is_https(&headers) { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        auth::SESSION_COOKIE,
        token,
        60 * 60 * 24 * 180,
        secure
    );
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true, "deviceName": device_name })),
    )
        .into_response()
}

async fn auth_me(State(state): State<Shared>, headers: HeaderMap) -> Response {
    let Some(token) = cookie_token(&headers) else {
        return Json(json!({ "authenticated": false })).into_response();
    };
    let state2 = state.clone();
    let device =
        tokio::task::spawn_blocking(move || auth::validate_session(&state2, &token)).await;
    match device {
        Ok(Some(id)) => Json(json!({ "authenticated": true, "deviceId": id })).into_response(),
        _ => Json(json!({ "authenticated": false })).into_response(),
    }
}

async fn auth_logout(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie_token(&headers) {
        let state2 = state.clone();
        let _ = tokio::task::spawn_blocking(move || auth::delete_session(&state2, &token)).await;
    }
    let cookie = format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        auth::SESSION_COOKIE
    );
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

// ---- コマンド呼び出し ----

async fn invoke_handler(
    State(state): State<Shared>,
    AxPath(cmd): AxPath<String>,
    Json(args): Json<Value>,
) -> Response {
    let state2 = state.clone();
    let cmd2 = cmd.clone();
    let result =
        tokio::task::spawn_blocking(move || dispatch(&state2, &cmd2, args, Origin::Web)).await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(msg)) => {
            let status = if msg.starts_with("CONFLICT:") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            err_json(status, &msg)
        }
        Err(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "内部エラーが発生しました"),
    }
}

// ---- SSE ----

async fn sse_handler(
    State(state): State<Shared>,
) -> sse::Sse<impl tokio_stream::Stream<Item = Result<sse::Event, std::convert::Infallible>>> {
    let rx = state.events.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|ev| match ev {
        Ok(ev) => sse::Event::default().json_data(&ev).ok().map(Ok),
        Err(_) => None,
    });
    sse::Sse::new(stream).keep_alive(
        sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(20))
            .text("ping"),
    )
}

// ---- ファイル配信 ----

fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains(':')
    {
        return false;
    }
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
            | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
            | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    );
    !reserved
}

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "zip" => "application/zip",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "wasm" => "application/wasm",
        "ico" => "image/x-icon",
        "txt" | "log" | "tex" | "sty" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn serve_file(path: &Path) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime_of(path).to_string()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'none'; style-src 'unsafe-inline'".to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => err_json(StatusCode::NOT_FOUND, "ファイルが見つかりません"),
    }
}

#[derive(serde::Deserialize, Default)]
struct GraphFileQuery {
    download: Option<u8>,
}

async fn file_graph(
    State(state): State<Shared>,
    AxPath((id, format)): AxPath<(String, String)>,
    Query(query): Query<GraphFileQuery>,
) -> Response {
    let path = match graphs::graph_file_path(&state, &id, &format) {
        Ok(path) => path,
        Err(message) if message.contains("見つかりません") => {
            return err_json(StatusCode::NOT_FOUND, &message)
        }
        Err(message) => return err_json(StatusCode::BAD_REQUEST, &message),
    };
    let mut response = serve_file(&path);
    if response.status() == StatusCode::OK {
        let disposition = if query.download == Some(1) {
            format!("attachment; filename=\"graph.{}\"", if format == "thumbnail" { "png" } else { &format })
        } else {
            "inline".to_string()
        };
        if let Ok(value) = disposition.parse() {
            response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
        }
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            "private, no-store".parse().expect("static header"),
        );
    }
    response
}

async fn file_attachment(State(state): State<Shared>, AxPath(name): AxPath<String>) -> Response {
    if !safe_name(&name) {
        return err_json(StatusCode::BAD_REQUEST, "不正なファイル名です");
    }
    serve_file(&state.attachments_dir().join(name))
}

async fn file_part_attachment(
    State(state): State<Shared>,
    AxPath(name): AxPath<String>,
) -> Response {
    if !safe_name(&name) {
        return err_json(StatusCode::BAD_REQUEST, "不正なファイル名です");
    }
    serve_file(&state.part_attachments_dir().join(name))
}

async fn file_template_asset(
    State(state): State<Shared>,
    AxPath((tid, name)): AxPath<(i64, String)>,
) -> Response {
    if !safe_name(&name) {
        return err_json(StatusCode::BAD_REQUEST, "不正なファイル名です");
    }
    serve_file(
        &state
            .data_dir
            .join("template_assets")
            .join(tid.to_string())
            .join(name),
    )
}

async fn file_ai_job(
    State(state): State<Shared>,
    AxPath((uuid, name)): AxPath<(String, String)>,
) -> Response {
    if !safe_name(&uuid) || !safe_name(&name) {
        return err_json(StatusCode::BAD_REQUEST, "不正なファイル名です");
    }
    serve_file(&state.ai_jobs_dir().join(uuid).join(name))
}

#[derive(serde::Deserialize)]
struct BuildFileQuery {
    path: String,
}

fn is_safe_local_absolute(path: &Path) -> bool {
    let raw = path.as_os_str().to_string_lossy();
    if raw.is_empty()
        || raw.len() > 1024
        || raw.starts_with(r"\\")
        || raw.starts_with("//")
        || raw.starts_with(r"\\?\")
        || raw.starts_with(r"\\.\")
        || !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    !raw
        .char_indices()
        .any(|(index, ch)| ch == ':' && index != 1)
}

fn path_starts_with_case_insensitive(path: &Path, root: &Path) -> bool {
    let path = path
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    let root = root
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    path == root || path.starts_with(&(root + "\\"))
}

/// 生成済み成果物として配信・読込を許可するルート
/// （一時ビルドフォルダ / 出力フォルダ / AIジョブフォルダ / 既定・設定出力先）
pub(crate) fn compiled_file_roots(state: &AppState) -> Vec<PathBuf> {
    let mut allowed_roots: Vec<PathBuf> = vec![
        std::env::temp_dir().join("kyozai-kobo-build"),
        state.data_dir.join("output"),
    ];
    allowed_roots.push(state.ai_jobs_dir());
    if let Some(docs) = &state.documents_dir {
        allowed_roots.push(docs.join("教材工房"));
    }
    // デスクトップで明示設定されたローカル出力先。ここでは作成もアクセスもしない。
    if let Ok(conn) = state.conn.lock() {
        let custom: Option<String> = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key='output_dir'",
                [],
                |row| row.get(0),
            )
            .ok();
        if let Some(custom) = custom {
            let root = PathBuf::from(custom);
            if is_safe_local_absolute(&root) {
                allowed_roots.push(root);
            }
        }
    }
    allowed_roots
}

pub(crate) enum CompiledFileError {
    Forbidden(&'static str),
    NotFound(&'static str),
}

impl CompiledFileError {
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Forbidden(m) | Self::NotFound(m) => m,
        }
    }
}

/// コンパイル成果物パスを許可ルート配下で検証し、正規化して返す。
/// HTTPの/api/files/buildとデスクトップのread_compiled_fileで共用する。
pub(crate) fn resolve_compiled_file(
    state: &AppState,
    requested: &Path,
) -> Result<PathBuf, CompiledFileError> {
    if !is_safe_local_absolute(requested) {
        return Err(CompiledFileError::Forbidden(
            "ローカルの絶対パスだけを指定できます",
        ));
    }
    let allowed_roots = compiled_file_roots(state);
    // 許可ルート外のパスはcanonicalize前に拒否し、UNC/SMB等へのアクセスを誘発しない。
    if !allowed_roots
        .iter()
        .any(|root| path_starts_with_case_insensitive(requested, root))
    {
        return Err(CompiledFileError::Forbidden(
            "このパスへのアクセスは許可されていません",
        ));
    }
    let Ok(canonical) = requested.canonicalize() else {
        return Err(CompiledFileError::NotFound("ファイルが見つかりません"));
    };
    let permitted = allowed_roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| path_starts_with_case_insensitive(&canonical, &canonical_root))
            .unwrap_or(false)
    });
    if !permitted {
        return Err(CompiledFileError::Forbidden(
            "このパスへのアクセスは許可されていません",
        ));
    }
    Ok(canonical)
}

/// コンパイル成果物（一時ビルドフォルダ / 出力フォルダ / AIジョブフォルダ配下のみ）を配信
async fn file_build(
    State(state): State<Shared>,
    Query(q): Query<BuildFileQuery>,
) -> Response {
    match resolve_compiled_file(&state, &PathBuf::from(&q.path)) {
        Ok(canonical) => serve_file(&canonical),
        Err(CompiledFileError::NotFound(msg)) => err_json(StatusCode::NOT_FOUND, msg),
        Err(CompiledFileError::Forbidden(msg)) => err_json(StatusCode::FORBIDDEN, msg),
    }
}

// ---- アップロード ----

/// マジックバイトで画像/PDF形式を検証
fn sniff_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"%PDF") {
        return Some("pdf");
    }
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

fn validate_uploaded_image(bytes: &[u8]) -> Result<(), String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "画像形式を判定できません".to_string())?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "画像データが壊れているか、対応していない形式です".to_string())?;
    const MAX_SIDE: u32 = 12_000;
    const MAX_PIXELS: u64 = 50_000_000;
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(format!(
            "画像寸法が大きすぎます（{}x{}、上限{}画素）",
            width, height, MAX_PIXELS
        ));
    }
    Ok(())
}

async fn read_upload(
    multipart: &mut Multipart,
) -> Result<(String, Vec<u8>), String> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| format!("アップロードの読み取りに失敗: {}", e))?
    {
        if field.name() == Some("file") {
            let file_name = field.file_name().unwrap_or("upload").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| format!("アップロードの読み取りに失敗: {}", e))?;
            return Ok((file_name, bytes.to_vec()));
        }
    }
    Err("file フィールドがありません".into())
}

/// アップロードを検証して一時保存し、(一時パス, 元ファイル名) を返す
fn store_upload(
    state: &AppState,
    file_name: &str,
    bytes: &[u8],
    allow_pdf: bool,
) -> Result<(PathBuf, String), String> {
    let Some(kind) = sniff_kind(bytes) else {
        return Err("対応形式は PNG / JPEG / WEBP / PDF です（ファイル内容を確認してください）".into());
    };
    if kind == "pdf" && !allow_pdf {
        return Err("この用途ではPDFは使用できません".into());
    }
    if kind != "pdf" {
        validate_uploaded_image(bytes)?;
    }
    let ext = kind;
    let safe_original: String = Path::new(file_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload".into());
    let stored = format!(
        "up{}.{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12],
        ext
    );
    let dest = state.uploads_dir().join(&stored);
    std::fs::write(&dest, bytes).map_err(|e| format!("保存に失敗しました: {}", e))?;
    Ok((dest, safe_original))
}

#[derive(serde::Deserialize)]
struct ProblemIdQuery {
    #[serde(rename = "problemId")]
    problem_id: i64,
}

async fn upload_attachment(
    State(state): State<Shared>,
    Query(q): Query<ProblemIdQuery>,
    mut multipart: Multipart,
) -> Response {
    let (file_name, bytes) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
    };
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (tmp, original) = store_upload(&state2, &file_name, &bytes, true)?;
        // 元ファイル名を維持しつつ既存サービスへ渡す（コピー後に一時ファイル削除）
        let renamed = tmp.with_file_name(format!(
            "up{}_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..6],
            sanitize_upload_name(&original, &tmp)
        ));
        let src = if std::fs::rename(&tmp, &renamed).is_ok() { renamed } else { tmp };
        let r = crate::commands::attachments::add_attachment(
            &state2,
            q.problem_id,
            src.to_string_lossy().to_string(),
        );
        std::fs::remove_file(&src).ok();
        r.map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
    })
    .await;
    match result {
        Ok(Ok(v)) => {
            state.emit("problems", "add_attachment", json!({"problemId": q.problem_id}));
            Json(v).into_response()
        }
        Ok(Err(e)) => err_json(StatusCode::BAD_REQUEST, &e),
        Err(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "内部エラー"),
    }
}

#[derive(serde::Deserialize)]
struct PartIdQuery {
    #[serde(rename = "partId")]
    part_id: i64,
}

async fn upload_part_attachment(
    State(state): State<Shared>,
    Query(q): Query<PartIdQuery>,
    mut multipart: Multipart,
) -> Response {
    let (file_name, bytes) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(e) => return err_json(StatusCode::BAD_REQUEST, &e),
    };
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (tmp, original) = store_upload(&state2, &file_name, &bytes, true)?;
        let renamed = tmp.with_file_name(format!(
            "up{}_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..6],
            sanitize_upload_name(&original, &tmp)
        ));
        let src = if std::fs::rename(&tmp, &renamed).is_ok() { renamed } else { tmp };
        let r = crate::commands::parts::add_part_attachment(
            &state2,
            q.part_id,
            src.to_string_lossy().to_string(),
        );
        std::fs::remove_file(&src).ok();
        r.map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
    })
    .await;
    match result {
        Ok(Ok(v)) => {
            state.emit("parts", "add_part_attachment", json!({"partId": q.part_id}));
            Json(v).into_response()
        }
        Ok(Err(e)) => err_json(StatusCode::BAD_REQUEST, &e),
        Err(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "内部エラー"),
    }
}

/// 元ファイル名から拡張子を保ちつつ安全な名前を作る
fn sanitize_upload_name(original: &str, tmp: &Path) -> String {
    let ext = tmp
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "bin".into());
    let stem: String = Path::new(original)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into())
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || (*c as u32) > 0x7F)
        .take(40)
        .collect();
    let stem = if stem.is_empty() { "file".to_string() } else { stem };
    format!("{}.{}", stem, ext)
}

// ---- 静的ファイル（埋め込みdist） ----

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = DIST
        .get_file(path)
        .or_else(|| {
            // SPAフォールバック（/api以外）
            if path.starts_with("api/") {
                None
            } else {
                DIST.get_file("index.html")
            }
        });
    match file {
        Some(f) => {
            let mime = mime_of(Path::new(f.path()));
            let cache = if path.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    (header::CACHE_CONTROL, cache.to_string()),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                    (header::X_FRAME_OPTIONS, "DENY".to_string()),
                    (
                        header::REFERRER_POLICY,
                        "no-referrer".to_string(),
                    ),
                    (
                        header::CONTENT_SECURITY_POLICY,
                        "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; worker-src 'self' blob:; manifest-src 'self'"
                            .to_string(),
                    ),
                ],
                f.contents().to_vec(),
            )
                .into_response()
        }
        None => err_json(StatusCode::NOT_FOUND, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::mime_of;
    use std::path::Path;

    #[test]
    fn module_worker_uses_javascript_mime() {
        assert_eq!(
            mime_of(Path::new("assets/pdf.worker.min.mjs")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_of(Path::new("pdfjs/wasm/qcms_bg.wasm")), "application/wasm");
    }
}
