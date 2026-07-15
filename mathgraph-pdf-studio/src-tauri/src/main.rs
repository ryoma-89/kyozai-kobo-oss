// リリースビルドで余計なコンソールウィンドウを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: i64 = 1;
const APP_NAME: &str = "MathGraph PDF Studio";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationRequestFile {
    protocol_version: i64,
    request_id: String,
    return_folder: String,
    mode: Option<String>,
    editable_source: Option<Value>,
    update_asset_id: Option<Value>,
    latex_insert_options: Option<LatexInsertOptions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatexInsertOptions {
    width: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationRequestInfo {
    request_id: String,
    request_path: String,
    return_folder: String,
    mode: String,
    update_asset_id: Option<String>,
    latex_width: Option<String>,
    initial_project_json: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteIntegrationPayload {
    request_path: String,
    graph_id: String,
    display_name: String,
    pdf_base64: String,
    png_base64: String,
    thumbnail_base64: Option<String>,
    graph_json: String,
    graph_tex: String,
    graph_type: Option<String>,
}

/// バイナリファイルを保存する（PDF/PNG など。中身は base64 で受け取る）
#[tauri::command]
fn write_file(path: String, contents_base64: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(contents_base64)
        .map_err(|e| format!("データの変換に失敗しました: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("ファイルを保存できませんでした: {e}"))
}

/// テキストファイルを保存する（プロジェクト JSON / SVG など）
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("ファイルを保存できませんでした: {e}"))
}

/// テキストファイルを読み込む（プロジェクト JSON）
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("ファイルを読み込めませんでした: {e}"))
}

fn is_safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn integration_arg_path() -> Option<PathBuf> {
    let mut args = std::env::args_os();
    while let Some(arg) = args.next() {
        if arg == "--integration-request" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn read_request_file(path: &Path) -> Result<IntegrationRequestFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("integration request could not be read: {e}"))?;
    let req: IntegrationRequestFile =
        serde_json::from_str(&text).map_err(|e| format!("invalid integration request JSON: {e}"))?;
    if req.protocol_version != PROTOCOL_VERSION {
        return Err("unsupported integration protocol version".into());
    }
    if !is_safe_token(&req.request_id) {
        return Err("requestId contains unsafe characters".into());
    }
    let return_folder = PathBuf::from(&req.return_folder);
    if !return_folder.is_absolute() {
        return Err("returnFolder must be an absolute path".into());
    }
    Ok(req)
}

fn request_editable_source(req: &IntegrationRequestFile) -> Option<String> {
    match req.editable_source.as_ref()? {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn request_update_asset_id(req: &IntegrationRequestFile) -> Option<String> {
    match req.update_asset_id.as_ref()? {
        Value::String(s) if is_safe_token(s) => Some(s.clone()),
        _ => None,
    }
}

fn decode_base64(data: &str, label: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("{label} could not be decoded: {e}"))
}

fn write_manifest(return_folder: &Path, manifest: Value) -> Result<(), String> {
    std::fs::write(
        return_folder.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("manifest JSON failed: {e}"))?,
    )
    .map_err(|e| format!("manifest could not be written: {e}"))
}

#[tauri::command]
fn get_integration_request() -> Result<Option<IntegrationRequestInfo>, String> {
    let Some(path) = integration_arg_path() else {
        return Ok(None);
    };
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("integration request path is invalid: {e}"))?;
    let req = read_request_file(&canonical)?;
    let initial_project_json = match request_editable_source(&req) {
        Some(source) => {
            let source_path = PathBuf::from(&source);
            if !source_path.is_absolute() || !source_path.exists() {
                return Err("editable graph source was not found".into());
            }
            Some(
                std::fs::read_to_string(source_path)
                    .map_err(|e| format!("editable graph source could not be read: {e}"))?,
            )
        }
        None => None,
    };
    let update_asset_id = request_update_asset_id(&req);
    let mode = req.mode.clone().unwrap_or_else(|| "insert".into());
    let latex_width = req.latex_insert_options.as_ref().and_then(|o| o.width.clone());
    Ok(Some(IntegrationRequestInfo {
        request_id: req.request_id,
        request_path: canonical.to_string_lossy().to_string(),
        return_folder: req.return_folder,
        mode,
        update_asset_id,
        latex_width,
        initial_project_json,
    }))
}

#[tauri::command]
fn complete_integration(payload: CompleteIntegrationPayload) -> Result<(), String> {
    let request_path = PathBuf::from(&payload.request_path)
        .canonicalize()
        .map_err(|e| format!("integration request path is invalid: {e}"))?;
    let req = read_request_file(&request_path)?;
    let return_folder = PathBuf::from(&req.return_folder);
    std::fs::create_dir_all(&return_folder)
        .map_err(|e| format!("return folder could not be created: {e}"))?;

    let write_result = (|| -> Result<(), String> {
        std::fs::write(
            return_folder.join("graph.pdf"),
            decode_base64(&payload.pdf_base64, "PDF")?,
        )
        .map_err(|e| format!("graph.pdf could not be written: {e}"))?;
        std::fs::write(
            return_folder.join("graph.png"),
            decode_base64(&payload.png_base64, "PNG")?,
        )
        .map_err(|e| format!("graph.png could not be written: {e}"))?;
        std::fs::write(return_folder.join("graph.json"), payload.graph_json)
            .map_err(|e| format!("graph.json could not be written: {e}"))?;
        std::fs::write(return_folder.join("graph.tex"), payload.graph_tex)
            .map_err(|e| format!("graph.tex could not be written: {e}"))?;
        if let Some(thumbnail) = payload.thumbnail_base64.as_deref() {
            std::fs::write(
                return_folder.join("thumbnail.png"),
                decode_base64(thumbnail, "thumbnail")?,
            )
            .map_err(|e| format!("thumbnail.png could not be written: {e}"))?;
        } else {
            std::fs::copy(return_folder.join("graph.png"), return_folder.join("thumbnail.png"))
                .map_err(|e| format!("thumbnail.png could not be prepared: {e}"))?;
        }
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = write_manifest(
            &return_folder,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "requestId": req.request_id,
                "status": "failed",
                "error": {
                    "message": "Graph export failed.",
                    "details": e
                }
            }),
        );
        return Err(e);
    }

    let graph_id = if is_safe_token(&payload.graph_id) {
        payload.graph_id
    } else {
        format!("graph_{}", uuid::Uuid::new_v4().simple())
    };
    let update_asset_id = request_update_asset_id(&req);
    let width = req
        .latex_insert_options
        .as_ref()
        .and_then(|o| o.width.clone())
        .unwrap_or_else(|| "0.72\\linewidth".into());
    let suggested_latex = format!(
        "\\begin{{center}}\n  \\includegraphics[width={}]{{assets/graphs/{}/graph.pdf}}\n\\end{{center}}",
        width, graph_id
    );
    let display_name = if payload.display_name.trim().is_empty() {
        "Graph".to_string()
    } else {
        payload.display_name
    };

    write_manifest(
        &return_folder,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "requestId": req.request_id,
            "status": "completed",
            "createdAt": chrono::Local::now().to_rfc3339(),
            "graph": {
                "graphId": graph_id,
                "displayName": display_name,
                "primaryAsset": "graph.pdf",
                "previewAsset": "graph.png",
                "thumbnailAsset": "thumbnail.png",
                "editableSource": "graph.json",
                "latexSource": "graph.tex"
            },
            "suggestedLatex": suggested_latex,
            "metadata": {
                "graphType": payload.graph_type.unwrap_or_else(|| "function".into()),
                "sourceApp": APP_NAME,
                "sourceVersion": env!("CARGO_PKG_VERSION"),
                "updateAssetId": update_asset_id
            }
        }),
    )
}

#[tauri::command]
fn cancel_integration(request_path: String) -> Result<(), String> {
    let request_path = PathBuf::from(request_path)
        .canonicalize()
        .map_err(|e| format!("integration request path is invalid: {e}"))?;
    let req = read_request_file(&request_path)?;
    let return_folder = PathBuf::from(&req.return_folder);
    std::fs::create_dir_all(&return_folder)
        .map_err(|e| format!("return folder could not be created: {e}"))?;
    write_manifest(
        &return_folder,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "requestId": req.request_id,
            "status": "cancelled",
            "cancelledAt": chrono::Local::now().to_rfc3339()
        }),
    )
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            write_file,
            write_text_file,
            read_text_file,
            get_integration_request,
            complete_integration,
            cancel_integration
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
