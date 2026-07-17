use crate::db::now_str;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROTOCOL_VERSION: i64 = 1;
const SOURCE_APP: &str = "KyouzaiKobo";
const TARGET_APP: &str = "MathGraph PDF Studio";
const MAX_ASSET_BYTES: u64 = 80 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartGraphIntegrationPayload {
    pub project_id: Option<i64>,
    pub problem_id: Option<i64>,
    pub item_id: Option<i64>,
    pub insert_target: String,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub reedit_asset_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphIntegrationSession {
    pub request_id: String,
    pub request_path: String,
    pub return_folder: String,
    pub graph_app_path: String,
    pub launched: bool,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphIntegrationPoll {
    pub status: String,
    pub request_id: String,
    pub asset_id: Option<String>,
    pub graph_id: Option<String>,
    pub display_name: Option<String>,
    pub inserted_latex: Option<String>,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphAssetSummary {
    pub asset_id: String,
    pub graph_id: String,
    pub display_name: String,
    pub project_id: Option<i64>,
    pub problem_id: Option<i64>,
    pub item_id: Option<i64>,
    pub source_application: String,
    pub editable_source_path: String,
    pub primary_asset_path: String,
    pub preview_asset_path: String,
    pub latex_source_path: String,
    pub inserted_latex: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphIntegrationTestResult {
    pub ok: bool,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationRequest {
    protocol_version: i64,
    request_id: String,
    target: RequestTarget,
    return_folder: String,
    latex_insert_options: Option<LatexInsertOptions>,
    update_asset_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestTarget {
    project_id: Option<i64>,
    problem_id: Option<i64>,
    item_id: Option<i64>,
    insert_target: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatexInsertOptions {
    width: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationManifest {
    protocol_version: Option<i64>,
    request_id: String,
    status: String,
    graph: Option<ManifestGraph>,
    metadata: Option<Value>,
    error: Option<ManifestError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestGraph {
    graph_id: Option<String>,
    display_name: Option<String>,
    primary_asset: Option<String>,
    preview_asset: Option<String>,
    thumbnail_asset: Option<String>,
    editable_source: Option<String>,
    latex_source: Option<String>,
}

#[derive(Deserialize)]
struct ManifestError {
    message: Option<String>,
    details: Option<String>,
}

pub(crate) fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", params![key], |r| r.get(0))
        .ok()
        .filter(|v: &String| !v.trim().is_empty())
}

fn is_safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(crate) fn safe_width(value: Option<&str>) -> String {
    let v = value.unwrap_or("0.72\\linewidth").trim();
    let ok = !v.is_empty()
        && v.len() <= 40
        && !v.contains('\n')
        && !v.contains('\r')
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '.' | '\\' | '{' | '}' | '_' | '-' | '+' | '/' | ' ')
        });
    if ok {
        v.to_string()
    } else {
        "0.72\\linewidth".to_string()
    }
}

fn integration_root(conn: &Connection, state: &AppState) -> PathBuf {
    if let Some(path) = get_setting(conn, "graph_integration_dir") {
        let pb = PathBuf::from(path);
        if pb.is_absolute() {
            fs::create_dir_all(&pb).ok();
            return pb;
        }
    }
    let pb = state.data_dir.join("integrations");
    fs::create_dir_all(&pb).ok();
    pb
}

fn candidate_graph_paths(state: &AppState) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.as_path();
        for _ in 0..8 {
            if let Some(parent) = cur.parent() {
                roots.push(parent.to_path_buf());
                cur = parent;
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd.as_path();
        for _ in 0..8 {
            roots.push(cur.to_path_buf());
            if let Some(parent) = cur.parent() {
                cur = parent;
            } else {
                break;
            }
        }
    }
    if let Some(resource) = &state.resource_dir {
        roots.push(resource.clone());
    }
    if cfg!(windows) {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            let base = PathBuf::from(program_files);
            roots.push(base.join("MathGraph PDF Studio"));
            roots.push(base.join("mathgraph-pdf-studio"));
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            let base = PathBuf::from(program_files_x86);
            roots.push(base.join("MathGraph PDF Studio"));
            roots.push(base.join("mathgraph-pdf-studio"));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data);
            roots.push(base.join("Programs").join("MathGraph PDF Studio"));
            roots.push(base.join("Programs").join("mathgraph-pdf-studio"));
            roots.push(base.join("MathGraph PDF Studio"));
            roots.push(base.join("mathgraph-pdf-studio"));
        }
    }

    let mut out = Vec::new();
    for root in roots {
        out.push(root.join("mathgraph-pdf-studio.exe"));
        out.push(root.join("MathGraph PDF Studio.exe"));
        out.push(root.join("mathgraph-pdf-studio").join("mathgraph-pdf-studio.exe"));
        out.push(
            root.join("mathgraph-pdf-studio")
                .join("src-tauri")
                .join("target")
                .join("release")
                .join("mathgraph-pdf-studio.exe"),
        );
        out.push(
            root.join("mathgraph-pdf-studio")
                .join("src-tauri")
                .join("target")
                .join("debug")
                .join("mathgraph-pdf-studio.exe"),
        );
    }
    out
}

fn is_graph_executable(path: &Path) -> bool {
    path.exists()
        && path
            .extension()
            .map(|e| e.to_string_lossy().eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
}

fn is_debug_graph_executable(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    normalized.contains("\\target\\debug\\")
}

fn detect_graph_app_candidate(state: &AppState) -> Option<PathBuf> {
    let candidates = candidate_graph_paths(state);
    candidates
        .iter()
        .find(|p| is_graph_executable(p) && !is_debug_graph_executable(p))
        .cloned()
        .or_else(|| candidates.into_iter().find(|p| is_graph_executable(p)))
}

fn resolve_graph_app_path(state: &AppState, conn: &Connection) -> Option<PathBuf> {
    if let Some(path) = get_setting(conn, "graph_app_path") {
        let pb = PathBuf::from(path);
        if is_graph_executable(&pb) {
            if is_debug_graph_executable(&pb) {
                return detect_graph_app_candidate(state).or(Some(pb));
            }
            return Some(pb);
        }
    }
    detect_graph_app_candidate(state)
}

fn graph_asset_record(conn: &Connection, asset_id: &str) -> Result<Option<GraphAssetSummary>, String> {
    conn.query_row(
        "SELECT asset_id, graph_id, display_name, project_id, problem_id, item_id,
                source_application, editable_source_path, primary_asset_path, preview_asset_path,
                latex_source_path, inserted_latex, created_at, updated_at, version
         FROM graph_assets WHERE asset_id=?1",
        params![asset_id],
        |r| {
            Ok(GraphAssetSummary {
                asset_id: r.get(0)?,
                graph_id: r.get(1)?,
                display_name: r.get(2)?,
                project_id: r.get(3)?,
                problem_id: r.get(4)?,
                item_id: r.get(5)?,
                source_application: r.get(6)?,
                editable_source_path: r.get(7)?,
                primary_asset_path: r.get(8)?,
                preview_asset_path: r.get(9)?,
                latex_source_path: r.get(10)?,
                inserted_latex: r.get(11)?,
                created_at: r.get(12)?,
                updated_at: r.get(13)?,
                version: r.get(14)?,
            })
        },
    )
    .optional()
    .map_err(err_str)
}

fn safe_manifest_file_name(name: Option<&str>, field: &str, allowed_exts: &[&str]) -> Result<String, String> {
    let name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("manifest is missing {field}"))?;
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(format!("manifest {field} must be a file name"));
    }
    let path = Path::new(name);
    if path.file_name().and_then(|n| n.to_str()) != Some(name) {
        return Err(format!("manifest {field} must not contain a path"));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !allowed_exts.iter().any(|e| *e == ext) {
        return Err(format!("manifest {field} has an unsupported extension"));
    }
    Ok(name.to_string())
}

fn checked_source_file(return_folder: &Path, name: &str) -> Result<PathBuf, String> {
    let root = return_folder.canonicalize().map_err(err_str)?;
    let src = root.join(name);
    let canonical = src.canonicalize().map_err(|e| format!("asset not found: {name}: {e}"))?;
    if !canonical.starts_with(&root) {
        return Err("asset path escapes the integration session".into());
    }
    let meta = fs::metadata(&canonical).map_err(err_str)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_ASSET_BYTES {
        return Err(format!("asset has an invalid size: {name}"));
    }
    Ok(canonical)
}

fn copy_named_asset(
    return_folder: &Path,
    manifest_name: Option<&str>,
    field: &str,
    allowed_exts: &[&str],
    dest_dir: &Path,
    dest_base: &str,
    required: bool,
) -> Result<Option<PathBuf>, String> {
    let Some(name) = manifest_name else {
        if required {
            return Err(format!("manifest is missing {field}"));
        }
        return Ok(None);
    };
    let safe_name = safe_manifest_file_name(Some(name), field, allowed_exts)?;
    let source = checked_source_file(return_folder, &safe_name)?;
    let ext = Path::new(&safe_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let dest = dest_dir.join(format!("{dest_base}.{ext}"));
    fs::copy(source, &dest).map_err(|e| format!("failed to copy {field}: {e}"))?;
    Ok(Some(dest))
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest).map_err(err_str)?;
    for entry in fs::read_dir(src).map_err(err_str)? {
        let entry = entry.map_err(err_str)?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &dest_path)?;
        } else if path.is_file() {
            fs::copy(&path, &dest_path).map_err(err_str)?;
        }
    }
    Ok(())
}

fn make_latex(asset_id: &str, primary_name: &str, width: &str) -> String {
    format!(
        "\n\\noindent\\includegraphics[width={},height=0.28\\textheight,keepaspectratio]{{assets/graphs/{}/{}}}\\par\\smallskip\n",
        width, asset_id, primary_name
    )
}

fn read_request(request_path: &Path) -> Result<IntegrationRequest, String> {
    let text = fs::read_to_string(request_path).map_err(err_str)?;
    let request: IntegrationRequest = serde_json::from_str(&text).map_err(|e| format!("invalid request JSON: {e}"))?;
    if request.protocol_version != PROTOCOL_VERSION {
        return Err("unsupported integration protocol version".into());
    }
    if !is_safe_token(&request.request_id) {
        return Err("requestId contains unsafe characters".into());
    }
    Ok(request)
}

fn import_completed_manifest(
    conn: &Connection,
    state: &AppState,
    request: &IntegrationRequest,
    manifest: &IntegrationManifest,
) -> Result<GraphIntegrationPoll, String> {
    if manifest.protocol_version.unwrap_or(PROTOCOL_VERSION) != PROTOCOL_VERSION {
        return Err("unsupported manifest protocol version".into());
    }
    let graph = manifest
        .graph
        .as_ref()
        .ok_or_else(|| "completed manifest does not contain graph metadata".to_string())?;
    let graph_id = graph
        .graph_id
        .clone()
        .filter(|s| is_safe_token(s))
        .unwrap_or_else(|| format!("graph_{}", uuid::Uuid::new_v4().simple()));
    let display_name = graph
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Graph".to_string());
    let width = safe_width(
        request
            .latex_insert_options
            .as_ref()
            .and_then(|o| o.width.as_deref()),
    );
    let _insert_target = request.target.insert_target.as_str();

    let return_folder = PathBuf::from(&request.return_folder);
    if !return_folder.is_absolute() || !return_folder.exists() {
        return Err("request returnFolder is invalid".into());
    }

    let is_update = request
        .update_asset_id
        .as_deref()
        .filter(|id| is_safe_token(id))
        .is_some();
    let asset_id = request
        .update_asset_id
        .as_ref()
        .filter(|id| is_safe_token(id))
        .cloned()
        .unwrap_or_else(|| format!("graph_{}", uuid::Uuid::new_v4().simple()));
    let graph_root = state.graph_assets_dir();
    let asset_dir = graph_root.join(&asset_id);
    let temp_dir = graph_root.join(format!("{}.tmp_{}", asset_id, uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&temp_dir).map_err(err_str)?;

    let primary = copy_named_asset(
        &return_folder,
        graph.primary_asset.as_deref(),
        "graph.primaryAsset",
        &["pdf", "svg"],
        &temp_dir,
        "graph",
        true,
    )?
    .ok_or_else(|| "primary asset was not copied".to_string())?;
    let preview = copy_named_asset(
        &return_folder,
        graph.preview_asset.as_deref(),
        "graph.previewAsset",
        &["png", "jpg", "jpeg"],
        &temp_dir,
        "graph",
        true,
    )?
    .ok_or_else(|| "preview asset was not copied".to_string())?;
    let editable = copy_named_asset(
        &return_folder,
        graph.editable_source.as_deref(),
        "graph.editableSource",
        &["json"],
        &temp_dir,
        "graph",
        true,
    )?
    .ok_or_else(|| "editable source was not copied".to_string())?;
    let latex_source = copy_named_asset(
        &return_folder,
        graph.latex_source.as_deref(),
        "graph.latexSource",
        &["tex"],
        &temp_dir,
        "graph",
        false,
    )?;
    let _thumbnail = copy_named_asset(
        &return_folder,
        graph.thumbnail_asset.as_deref(),
        "graph.thumbnailAsset",
        &["png", "jpg", "jpeg"],
        &temp_dir,
        "thumbnail",
        false,
    )?;

    if is_update && asset_dir.exists() {
        let backup_dir = graph_root.join("history").join(format!(
            "{}_{}",
            asset_id,
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
        copy_dir_all(&asset_dir, &backup_dir)?;
    }
    fs::create_dir_all(&asset_dir).map_err(err_str)?;
    copy_dir_all(&temp_dir, &asset_dir)?;
    fs::remove_dir_all(&temp_dir).ok();

    let primary_name = primary
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("graph.pdf")
        .to_string();
    let preview_name = preview
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("graph.png")
        .to_string();
    let editable_name = editable
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("graph.json")
        .to_string();
    let latex_name = latex_source
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let inserted_latex = make_latex(&asset_id, &primary_name, &width);
    let metadata_json = serde_json::to_string(&manifest.metadata.clone().unwrap_or_else(|| json!({}))).map_err(err_str)?;
    let now = now_str();
    let created_at = conn
        .query_row(
            "SELECT created_at FROM graph_assets WHERE asset_id=?1",
            params![asset_id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(err_str)?
        .unwrap_or_else(|| now.clone());
    let old_version = conn
        .query_row(
            "SELECT version FROM graph_assets WHERE asset_id=?1",
            params![asset_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(err_str)?
        .unwrap_or(0);
    let version = old_version + 1;

    let primary_path = asset_dir.join(primary_name).to_string_lossy().to_string();
    let preview_path = asset_dir.join(preview_name).to_string_lossy().to_string();
    let editable_path = asset_dir.join(editable_name).to_string_lossy().to_string();
    let latex_path = if latex_name.is_empty() {
        String::new()
    } else {
        asset_dir.join(latex_name).to_string_lossy().to_string()
    };

    conn.execute(
        "INSERT INTO graph_assets (
            asset_id, graph_id, display_name, project_id, problem_id, item_id,
            source_application, editable_source_path, primary_asset_path, preview_asset_path,
            latex_source_path, inserted_latex, metadata_json, created_at, updated_at, version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(asset_id) DO UPDATE SET
            graph_id=excluded.graph_id,
            display_name=excluded.display_name,
            project_id=excluded.project_id,
            problem_id=excluded.problem_id,
            item_id=excluded.item_id,
            source_application=excluded.source_application,
            editable_source_path=excluded.editable_source_path,
            primary_asset_path=excluded.primary_asset_path,
            preview_asset_path=excluded.preview_asset_path,
            latex_source_path=excluded.latex_source_path,
            inserted_latex=excluded.inserted_latex,
            metadata_json=excluded.metadata_json,
            updated_at=excluded.updated_at,
            version=excluded.version",
        params![
            asset_id,
            graph_id,
            display_name,
            request.target.project_id,
            request.target.problem_id,
            request.target.item_id,
            TARGET_APP,
            editable_path,
            primary_path,
            preview_path,
            latex_path,
            inserted_latex,
            metadata_json,
            created_at,
            now,
            version,
        ],
    )
    .map_err(err_str)?;

    Ok(GraphIntegrationPoll {
        status: "completed".into(),
        request_id: request.request_id.clone(),
        asset_id: Some(asset_id),
        graph_id: Some(graph_id),
        display_name: Some(display_name),
        inserted_latex: Some(inserted_latex),
        message: if is_update {
            "Graph asset updated.".into()
        } else {
            "Graph asset imported.".into()
        },
        details: None,
    })
}

pub fn detect_graph_app_path(state: &AppState) -> Result<Option<String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    Ok(resolve_graph_app_path(state, &conn).map(|p| p.to_string_lossy().to_string()))
}

pub fn test_graph_integration_settings(state: &AppState) -> Result<GraphIntegrationTestResult, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let Some(path) = resolve_graph_app_path(state, &conn) else {
        return Ok(GraphIntegrationTestResult {
            ok: false,
            path: None,
            message: "Graph app executable was not found. Set graph_app_path in Settings.".into(),
        });
    };
    let ok = path.exists() && path.extension().map(|e| e == "exe").unwrap_or(false);
    Ok(GraphIntegrationTestResult {
        ok,
        path: Some(path.to_string_lossy().to_string()),
        message: if ok {
            "Graph app executable is available.".into()
        } else {
            "Configured graph app path is not a valid .exe file.".into()
        },
    })
}

pub fn start_graph_integration(
    state: &AppState,
    payload: StartGraphIntegrationPayload,
) -> Result<GraphIntegrationSession, String> {
    if !payload.insert_target.trim().is_empty()
        && payload
            .insert_target
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    {
        return Err("insert target contains unsafe characters".into());
    }

    let conn = state.conn.lock().map_err(err_str)?;
    let graph_app = resolve_graph_app_path(state, &conn)
        .ok_or_else(|| "Graph app executable was not found. Open Settings and set graph_app_path.".to_string())?;
    let root = integration_root(&conn, &state);
    let requests_dir = root.join("requests");
    let sessions_dir = root.join("sessions");
    fs::create_dir_all(&requests_dir).map_err(err_str)?;
    fs::create_dir_all(&sessions_dir).map_err(err_str)?;

    let request_id = format!("req_{}", uuid::Uuid::new_v4().simple());
    let return_folder = sessions_dir.join(&request_id);
    fs::create_dir_all(&return_folder).map_err(err_str)?;
    let request_path = requests_dir.join(format!("request_{}.json", request_id));

    let mut editable_source = Value::Null;
    let mut update_asset_id = Value::Null;
    if let Some(asset_id) = payload.reedit_asset_id.as_deref() {
        if !is_safe_token(asset_id) {
            return Err("asset id contains unsafe characters".into());
        }
        let record = graph_asset_record(&conn, asset_id)?
            .ok_or_else(|| "selected graph asset was not found".to_string())?;
        if !Path::new(&record.editable_source_path).exists() {
            return Err("editable graph source was not found".into());
        }
        editable_source = json!(record.editable_source_path);
        update_asset_id = json!(asset_id);
    }

    let preferred = get_setting(&conn, "graph_preferred_output").unwrap_or_else(|| "pdf".into());
    let width = get_setting(&conn, "graph_insert_width").unwrap_or_else(|| "0.72\\linewidth".into());
    let request = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "requestId": request_id,
        "sourceApp": SOURCE_APP,
        "mode": if payload.reedit_asset_id.is_some() { "reedit" } else { "insert" },
        "target": {
            "projectId": payload.project_id,
            "problemId": payload.problem_id,
            "itemId": payload.item_id,
            "insertTarget": payload.insert_target,
            "cursorContext": {
                "selectionStart": payload.selection_start.unwrap_or(0),
                "selectionEnd": payload.selection_end.unwrap_or(payload.selection_start.unwrap_or(0)),
            }
        },
        "returnFolder": return_folder.to_string_lossy(),
        "preferredOutput": {
            "primaryFormat": preferred,
            "includePngPreview": true,
            "includeEditableSource": true,
            "includeLatexSource": true
        },
        "latexInsertOptions": {
            "mode": "includegraphics",
            "width": safe_width(Some(&width)),
            "alignment": "center",
            "caption": ""
        },
        "editableSource": editable_source,
        "updateAssetId": update_asset_id,
        "createdAt": chrono::Local::now().to_rfc3339()
    });
    fs::write(
        &request_path,
        serde_json::to_string_pretty(&request).map_err(err_str)?,
    )
    .map_err(err_str)?;

    Command::new(&graph_app)
        .arg("--integration-request")
        .arg(&request_path)
        .spawn()
        .map_err(|e| format!("failed to launch graph app: {e}"))?;

    Ok(GraphIntegrationSession {
        request_id,
        request_path: request_path.to_string_lossy().to_string(),
        return_folder: return_folder.to_string_lossy().to_string(),
        graph_app_path: graph_app.to_string_lossy().to_string(),
        launched: true,
        message: "Graph app launched.".into(),
    })
}

pub fn poll_graph_integration(
    state: &AppState,
    request_id: String,
    request_path: String,
) -> Result<GraphIntegrationPoll, String> {
    if !is_safe_token(&request_id) {
        return Err("request id contains unsafe characters".into());
    }
    let request = read_request(Path::new(&request_path))?;
    if request.request_id != request_id {
        return Err("request id does not match the request file".into());
    }
    let manifest_path = PathBuf::from(&request.return_folder).join("manifest.json");
    if !manifest_path.exists() {
        return Ok(GraphIntegrationPoll {
            status: "pending".into(),
            request_id,
            asset_id: None,
            graph_id: None,
            display_name: None,
            inserted_latex: None,
            message: "Waiting for graph app output.".into(),
            details: None,
        });
    }

    let text = fs::read_to_string(&manifest_path).map_err(err_str)?;
    let manifest: IntegrationManifest =
        serde_json::from_str(&text).map_err(|e| format!("invalid manifest JSON: {e}"))?;
    if manifest.request_id != request.request_id {
        return Err("manifest requestId does not match".into());
    }

    match manifest.status.as_str() {
        "completed" => {
            let conn = state.conn.lock().map_err(err_str)?;
            import_completed_manifest(&conn, &state, &request, &manifest)
        }
        "cancelled" => Ok(GraphIntegrationPoll {
            status: "cancelled".into(),
            request_id: request.request_id,
            asset_id: None,
            graph_id: None,
            display_name: None,
            inserted_latex: None,
            message: "Graph insertion was cancelled.".into(),
            details: None,
        }),
        "failed" => {
            let err = manifest.error;
            Ok(GraphIntegrationPoll {
                status: "failed".into(),
                request_id: request.request_id,
                asset_id: None,
                graph_id: None,
                display_name: None,
                inserted_latex: None,
                message: err
                    .as_ref()
                    .and_then(|e| e.message.clone())
                    .unwrap_or_else(|| "Graph app reported a failure.".into()),
                details: err.and_then(|e| e.details),
            })
        }
        other => Err(format!("unsupported manifest status: {other}")),
    }
}

pub fn list_graph_assets(
    state: &AppState,
    project_id: Option<i64>,
    problem_id: Option<i64>,
) -> Result<Vec<GraphAssetSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut out = Vec::new();
    if let Some(problem_id) = problem_id {
        let mut stmt = conn
            .prepare(
                "SELECT asset_id, graph_id, display_name, project_id, problem_id, item_id,
                        source_application, editable_source_path, primary_asset_path, preview_asset_path,
                        latex_source_path, inserted_latex, created_at, updated_at, version
                 FROM graph_assets
                 WHERE problem_id=?1
                 ORDER BY updated_at DESC",
            )
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![problem_id], |r| {
                Ok(GraphAssetSummary {
                    asset_id: r.get(0)?,
                    graph_id: r.get(1)?,
                    display_name: r.get(2)?,
                    project_id: r.get(3)?,
                    problem_id: r.get(4)?,
                    item_id: r.get(5)?,
                    source_application: r.get(6)?,
                    editable_source_path: r.get(7)?,
                    primary_asset_path: r.get(8)?,
                    preview_asset_path: r.get(9)?,
                    latex_source_path: r.get(10)?,
                    inserted_latex: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                    version: r.get(14)?,
                })
            })
            .map_err(err_str)?;
        for row in rows {
            out.push(row.map_err(err_str)?);
        }
    } else if let Some(project_id) = project_id {
        let mut stmt = conn
            .prepare(
                "SELECT asset_id, graph_id, display_name, project_id, problem_id, item_id,
                        source_application, editable_source_path, primary_asset_path, preview_asset_path,
                        latex_source_path, inserted_latex, created_at, updated_at, version
                 FROM graph_assets
                 WHERE project_id=?1
                 ORDER BY updated_at DESC",
            )
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(GraphAssetSummary {
                    asset_id: r.get(0)?,
                    graph_id: r.get(1)?,
                    display_name: r.get(2)?,
                    project_id: r.get(3)?,
                    problem_id: r.get(4)?,
                    item_id: r.get(5)?,
                    source_application: r.get(6)?,
                    editable_source_path: r.get(7)?,
                    primary_asset_path: r.get(8)?,
                    preview_asset_path: r.get(9)?,
                    latex_source_path: r.get(10)?,
                    inserted_latex: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                    version: r.get(14)?,
                })
            })
            .map_err(err_str)?;
        for row in rows {
            out.push(row.map_err(err_str)?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT asset_id, graph_id, display_name, project_id, problem_id, item_id,
                        source_application, editable_source_path, primary_asset_path, preview_asset_path,
                        latex_source_path, inserted_latex, created_at, updated_at, version
                 FROM graph_assets
                 ORDER BY updated_at DESC",
            )
            .map_err(err_str)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(GraphAssetSummary {
                    asset_id: r.get(0)?,
                    graph_id: r.get(1)?,
                    display_name: r.get(2)?,
                    project_id: r.get(3)?,
                    problem_id: r.get(4)?,
                    item_id: r.get(5)?,
                    source_application: r.get(6)?,
                    editable_source_path: r.get(7)?,
                    primary_asset_path: r.get(8)?,
                    preview_asset_path: r.get(9)?,
                    latex_source_path: r.get(10)?,
                    inserted_latex: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                    version: r.get(14)?,
                })
            })
            .map_err(err_str)?;
        for row in rows {
            out.push(row.map_err(err_str)?);
        }
    }
    Ok(out)
}
