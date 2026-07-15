//! 教材工房の共通サーバー上で管理するグラフ正本と派生出力。
//! MathGraph PDF Studio の既存 Project JSON を正本として保持し、
//! Webから渡されたパスやコマンドは一切実行しない。

use crate::db::now_str;
use crate::state::{err_str, AppState};
use super::graph_integration::{get_setting, safe_width};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_GRAPH_JSON: usize = 2 * 1024 * 1024;
const MAX_EXPORT_TOTAL: usize = 32 * 1024 * 1024;
const MAX_TITLE_CHARS: usize = 200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSummary {
    pub id: String,
    pub title: String,
    pub graph_type: String,
    pub source_type: String,
    pub warnings: Vec<String>,
    pub thumbnail_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub usage_count: i64,
    pub saved_formats: Vec<String>,
    pub exports_current: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphFull {
    #[serde(flatten)]
    pub summary: GraphSummary,
    pub graph_json: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionSummary {
    pub id: i64,
    pub graph_id: String,
    pub title: String,
    pub version: i64,
    pub saved_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionFull {
    #[serde(flatten)]
    pub summary: GraphVersionSummary,
    pub graph_json: String,
    pub graph_type: String,
    pub source_type: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGraphPayload {
    pub id: String,
    pub title: String,
    pub graph_json: String,
    pub graph_type: Option<String>,
    pub source_type: Option<String>,
    pub warnings: Option<Vec<String>>,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGraphPayload {
    pub title: String,
    pub graph_json: String,
    pub graph_type: Option<String>,
    pub source_type: Option<String>,
    pub warnings: Option<Vec<String>>,
}

fn safe_graph_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn clean_title(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("タイトルは{}文字以内にしてください", MAX_TITLE_CHARS));
    }
    Ok(if value.is_empty() {
        "無題のグラフ".to_string()
    } else {
        value.to_string()
    })
}

fn safe_enum(value: Option<String>, allowed: &[&str], fallback: &str) -> String {
    value
        .filter(|v| allowed.contains(&v.as_str()))
        .unwrap_or_else(|| fallback.to_string())
}

fn finite_number(object: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("range.{} は有限の数値で指定してください", key))
}

fn reject_unknown_keys(
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed: &[&str],
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{path}.{key} は未対応の項目です"));
    }
    Ok(())
}

fn finite_vec3(value: Option<&Value>, path: &str) -> Result<(), String> {
    let values = value.and_then(Value::as_array).ok_or_else(|| format!("{path} は3要素の配列で指定してください"))?;
    if values.len() != 3 || values.iter().any(|value| value.as_f64().is_none_or(|number| !number.is_finite() || number.abs() > 1.0e6)) {
        return Err(format!("{path} は±1000000以内の有限な3次元座標で指定してください"));
    }
    Ok(())
}

fn bounded_number(value: Option<&Value>, path: &str, min: f64, max: f64) -> Result<f64, String> {
    let number = value.and_then(Value::as_f64).ok_or_else(|| format!("{path} は数値で指定してください"))?;
    if !number.is_finite() || !(min..=max).contains(&number) { return Err(format!("{path} が範囲外です")); }
    Ok(number)
}

fn is_hex_color(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn validate_vertex_names(value: Option<&Value>, path: &str) -> Result<(), String> {
    let Some(value) = value else { return Ok(()); };
    let names = value.as_array().ok_or_else(|| format!("{path} は配列で指定してください"))?;
    if names.len() > 100 || names.iter().any(|name| name.as_str().is_none_or(|text| text.chars().count() > 30 || text.chars().any(char::is_control))) {
        return Err(format!("{path} が不正です"));
    }
    Ok(())
}

fn validate_spatial_geometry(kind: &str, geometry: &serde_json::Map<String, Value>, path: &str) -> Result<(), String> {
    match kind {
        "cube" => {
            bounded_number(geometry.get("sideLength"), &format!("{path}.sideLength"), 0.01, 10_000.0)?;
            validate_vertex_names(geometry.get("vertexNames"), &format!("{path}.vertexNames"))?;
        }
        "cuboid" => {
            for key in ["width", "height", "depth"] { bounded_number(geometry.get(key), &format!("{path}.{key}"), 0.01, 10_000.0)?; }
            validate_vertex_names(geometry.get("vertexNames"), &format!("{path}.vertexNames"))?;
        }
        "prism" | "pyramid" | "cylinder" | "cone" => {
            bounded_number(geometry.get("radius"), &format!("{path}.radius"), 0.01, 10_000.0)?;
            bounded_number(geometry.get("height"), &format!("{path}.height"), 0.01, 10_000.0)?;
            let sides = geometry.get("sides").and_then(Value::as_i64).ok_or_else(|| format!("{path}.sides は整数で指定してください"))?;
            if !(3..=48).contains(&sides) { return Err(format!("{path}.sides が範囲外です")); }
            validate_vertex_names(geometry.get("vertexNames"), &format!("{path}.vertexNames"))?;
        }
        "sphere" => { bounded_number(geometry.get("radius"), &format!("{path}.radius"), 0.01, 10_000.0)?; }
        "surface3d" => {
            let expression = geometry.get("expression").and_then(Value::as_str).ok_or_else(|| format!("{path}.expression が必要です"))?;
            if expression.is_empty() || expression.chars().count() > 500 || expression.chars().any(char::is_control) { return Err(format!("{path}.expression が不正です")); }
            let x_min = bounded_number(geometry.get("xMin"), &format!("{path}.xMin"), -1_000_000.0, 1_000_000.0)?;
            let x_max = bounded_number(geometry.get("xMax"), &format!("{path}.xMax"), -1_000_000.0, 1_000_000.0)?;
            let y_min = bounded_number(geometry.get("yMin"), &format!("{path}.yMin"), -1_000_000.0, 1_000_000.0)?;
            let y_max = bounded_number(geometry.get("yMax"), &format!("{path}.yMax"), -1_000_000.0, 1_000_000.0)?;
            if x_min >= x_max || y_min >= y_max { return Err(format!("{path} の表示範囲が不正です")); }
            let resolution = geometry.get("resolution").and_then(Value::as_i64).ok_or_else(|| format!("{path}.resolution は整数で指定してください"))?;
            if !(4..=160).contains(&resolution) || geometry.get("wireframe").and_then(Value::as_bool).is_none() { return Err(format!("{path} のメッシュ設定が不正です")); }
        }
        "planarGraph3d" => {
            let expression = geometry.get("expression").and_then(Value::as_str).ok_or_else(|| format!("{path}.expression が必要です"))?;
            if expression.is_empty() || expression.chars().count() > 500 || expression.chars().any(char::is_control) { return Err(format!("{path}.expression が不正です")); }
            let x_min = bounded_number(geometry.get("xMin"), &format!("{path}.xMin"), -1_000_000.0, 1_000_000.0)?;
            let x_max = bounded_number(geometry.get("xMax"), &format!("{path}.xMax"), -1_000_000.0, 1_000_000.0)?;
            let y_min = bounded_number(geometry.get("yMin"), &format!("{path}.yMin"), -1_000_000.0, 1_000_000.0)?;
            let y_max = bounded_number(geometry.get("yMax"), &format!("{path}.yMax"), -1_000_000.0, 1_000_000.0)?;
            if x_min >= x_max || y_min >= y_max { return Err(format!("{path} の表示範囲が不正です")); }
            let resolution = geometry.get("resolution").and_then(Value::as_i64).ok_or_else(|| format!("{path}.resolution は整数で指定してください"))?;
            if !(12..=240).contains(&resolution) { return Err(format!("{path} の描画精度が不正です")); }
            let t_min = geometry.get("tMin").map(|_| bounded_number(geometry.get("tMin"), &format!("{path}.tMin"), -1_000_000.0, 1_000_000.0)).transpose()?;
            let t_max = geometry.get("tMax").map(|_| bounded_number(geometry.get("tMax"), &format!("{path}.tMax"), -1_000_000.0, 1_000_000.0)).transpose()?;
            if t_min.zip(t_max).is_some_and(|(min, max)| min >= max) { return Err(format!("{path} の媒介変数範囲が不正です")); }
            if geometry.get("fill").is_some_and(|value| !value.is_boolean()) { return Err(format!("{path}.fill が不正です")); }
            if geometry.get("plane").is_some_and(|value| !matches!(value.as_str(), Some("xy" | "xz" | "yz"))) { return Err(format!("{path}.plane が不正です")); }
        }
        "point3d" => finite_vec3(geometry.get("point"), &format!("{path}.point"))?,
        "segment3d" | "vector3d" => {
            finite_vec3(geometry.get("from"), &format!("{path}.from"))?;
            finite_vec3(geometry.get("to"), &format!("{path}.to"))?;
            if geometry.get("lineType").is_some_and(|value| !matches!(value.as_str(), Some("solid" | "dashed"))) { return Err(format!("{path}.lineType が不正です")); }
        }
        "polygon3d" => {
            let points = geometry.get("points").and_then(Value::as_array).ok_or_else(|| format!("{path}.points が必要です"))?;
            if !(3..=500).contains(&points.len()) { return Err(format!("{path}.points の要素数が不正です")); }
            for (index, point) in points.iter().enumerate() { finite_vec3(Some(point), &format!("{path}.points[{index}]"))?; }
        }
        "plane3d" | "sectionPlane" => {
            finite_vec3(geometry.get("point"), &format!("{path}.point"))?;
            finite_vec3(geometry.get("normal"), &format!("{path}.normal"))?;
            let normal = geometry.get("normal").and_then(Value::as_array).unwrap();
            let length_squared: f64 = normal.iter().map(|value| value.as_f64().unwrap().powi(2)).sum();
            if length_squared < 1.0e-18 { return Err(format!("{path}.normal はゼロベクトルにできません")); }
            bounded_number(geometry.get("size"), &format!("{path}.size"), 0.01, 10_000.0)?;
        }
        "label3d" => {
            finite_vec3(geometry.get("position"), &format!("{path}.position"))?;
            let text = geometry.get("text").and_then(Value::as_str).ok_or_else(|| format!("{path}.text が必要です"))?;
            if text.chars().count() > 1_000 || text.chars().any(char::is_control) { return Err(format!("{path}.text が不正です")); }
        }
        _ => return Err(format!("{path} の型が不正です")),
    }
    Ok(())
}

fn validate_spatial_value(value: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > 10 { return Err(format!("{path} の入れ子が深すぎます")); }
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(number) => {
            let value = number.as_f64().ok_or_else(|| format!("{path} の数値が不正です"))?;
            if !value.is_finite() || value.abs() > 1.0e6 { Err(format!("{path} の数値が範囲外です")) } else { Ok(()) }
        }
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            if text.len() > 2_000 || text.chars().any(char::is_control)
                || ["://", "javascript:", "file:", "powershell", "cmd.exe", "\\\\", "../"]
                    .iter().any(|bad| lower.contains(bad))
            { Err(format!("{path} の文字列が不正です")) } else { Ok(()) }
        }
        Value::Array(values) => {
            if values.len() > 2_000 { return Err(format!("{path} の要素数が多すぎます")); }
            for (index, value) in values.iter().enumerate() { validate_spatial_value(value, &format!("{path}[{index}]"), depth + 1)?; }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() > 200 { return Err(format!("{path} の項目数が多すぎます")); }
            for (key, value) in values {
                if key.len() > 100 || !key.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-')) {
                    return Err(format!("{path} の項目名が不正です"));
                }
                validate_spatial_value(value, &format!("{path}.{key}"), depth + 1)?;
            }
            Ok(())
        }
    }
}

fn validate_spatial_graph(root: &serde_json::Map<String, Value>) -> Result<(), String> {
    reject_unknown_keys(root, "graph.json", &["schemaVersion", "documentType", "id", "title", "projection", "output", "scene", "objects", "createdAt", "updatedAt", "version"])?;
    if root.get("schemaVersion").and_then(Value::as_i64) != Some(2)
        || root.get("documentType").and_then(Value::as_str) != Some("spatial-geometry")
    { return Err("未対応の空間図形JSONです".into()); }
    let id = root.get("id").and_then(Value::as_str).ok_or_else(|| "空間図形IDが必要です".to_string())?;
    if !safe_graph_id(id) && !(id.starts_with("document_") && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        return Err("空間図形IDが不正です".into());
    }
    if root.get("title").and_then(Value::as_str).is_none_or(|title| title.chars().count() > 200) { return Err("タイトルが不正です".into()); }
    validate_spatial_value(root.get("title").unwrap_or(&Value::Null), "title", 0)?;
    let projection = root.get("projection").and_then(Value::as_object).ok_or_else(|| "projectionが必要です".to_string())?;
    reject_unknown_keys(projection, "projection", &["type", "cameraPosition", "target", "up", "zoom", "fov", "viewHeight", "preset"])?;
    if !matches!(projection.get("type").and_then(Value::as_str), Some("orthographic" | "perspective")) { return Err("投影方式が不正です".into()); }
    finite_vec3(projection.get("cameraPosition"), "projection.cameraPosition")?;
    finite_vec3(projection.get("target"), "projection.target")?;
    finite_vec3(projection.get("up"), "projection.up")?;
    if projection.get("zoom").and_then(Value::as_f64).is_none_or(|value| !(0.05..=100.0).contains(&value)) { return Err("zoomが不正です".into()); }
    if projection.get("fov").and_then(Value::as_f64).is_none_or(|value| !(10.0..=100.0).contains(&value)) { return Err("fovが不正です".into()); }
    if projection.contains_key("viewHeight") { bounded_number(projection.get("viewHeight"), "projection.viewHeight", 0.01, 5_000_000.0)?; }
    if let Some(output) = root.get("output") {
        let output = output.as_object().ok_or_else(|| "outputが不正です".to_string())?;
        reject_unknown_keys(output, "output", &["widthMm", "heightMm", "pixelWidth"])?;
        bounded_number(output.get("widthMm"), "output.widthMm", 10.0, 1_000.0)?;
        bounded_number(output.get("heightMm"), "output.heightMm", 10.0, 1_000.0)?;
        let pixel_width = output.get("pixelWidth").and_then(Value::as_i64).ok_or_else(|| "output.pixelWidthは整数で指定してください".to_string())?;
        if !(400..=8_000).contains(&pixel_width) { return Err("output.pixelWidthが範囲外です".into()); }
    }
    let scene = root.get("scene").and_then(Value::as_object).ok_or_else(|| "sceneが必要です".to_string())?;
    reject_unknown_keys(scene, "scene", &["background", "showAxes", "axesColor", "axesLabelSize", "axesLabelGap", "axesLabels", "axesLabelBackground", "showOriginLabel", "originLabel", "originLabelPosition", "originLabelOffset", "showGrid", "showHiddenEdges", "quality"])?;
    if !matches!(scene.get("background").and_then(Value::as_str), Some("white" | "transparent"))
        || !matches!(scene.get("quality").and_then(Value::as_str), Some("low" | "standard" | "high"))
        || scene.get("axesColor").is_some_and(|_| !is_hex_color(scene.get("axesColor")))
        || ["showAxes", "showGrid", "showHiddenEdges"].iter().any(|key| scene.get(*key).and_then(Value::as_bool).is_none())
    { return Err("sceneの表示設定が不正です".into()); }
    if scene.contains_key("axesLabelSize") { bounded_number(scene.get("axesLabelSize"), "scene.axesLabelSize", 8.0, 72.0)?; }
    if scene.contains_key("axesLabelGap") { bounded_number(scene.get("axesLabelGap"), "scene.axesLabelGap", 0.0, 200.0)?; }
    if let Some(labels_value) = scene.get("axesLabels") {
        let labels = labels_value.as_object().ok_or_else(|| "scene.axesLabelsはオブジェクトで指定してください".to_string())?;
        reject_unknown_keys(labels, "scene.axesLabels", &["x", "y", "z"])?;
        for axis in ["x", "y", "z"] {
            if labels.get(axis).and_then(Value::as_str).is_none_or(|text| text.chars().count() > 30 || text.chars().any(char::is_control)) {
                return Err(format!("scene.axesLabels.{axis}が不正です"));
            }
        }
    }
    if scene.get("axesLabelBackground").is_some_and(|value| !matches!(value.as_str(), Some("transparent" | "white"))) { return Err("scene.axesLabelBackgroundが不正です".into()); }
    if scene.get("showOriginLabel").is_some_and(|value| !value.is_boolean()) { return Err("scene.showOriginLabelが不正です".into()); }
    if scene.get("originLabel").is_some_and(|value| value.as_str().is_none_or(|text| text.chars().count() > 30 || text.chars().any(char::is_control))) { return Err("scene.originLabelが不正です".into()); }
    if scene.contains_key("originLabelPosition") { finite_vec3(scene.get("originLabelPosition"), "scene.originLabelPosition")?; }
    // originLabelOffsetは直前版の画面px形式。旧JSON読込専用として受理する。
    if let Some(offset) = scene.get("originLabelOffset") {
        let values = offset.as_array().ok_or_else(|| "scene.originLabelOffsetは2要素の配列で指定してください".to_string())?;
        if values.len() != 2 || values.iter().any(|value| value.as_f64().is_none_or(|number| !number.is_finite() || !(-500.0..=500.0).contains(&number))) {
            return Err("scene.originLabelOffsetが範囲外です".into());
        }
    }
    if root.get("version").and_then(Value::as_i64).is_none_or(|value| value < 1) { return Err("空間図形versionが不正です".into()); }
    for key in ["createdAt", "updatedAt"] {
        if root.get(key).and_then(Value::as_str).is_none_or(|value| value.len() > 100 || value.chars().any(char::is_control)) { return Err(format!("{key}が不正です")); }
    }
    let objects = root.get("objects").and_then(Value::as_array).ok_or_else(|| "objects配列が必要です".to_string())?;
    if objects.len() > 1_000 { return Err("空間図形オブジェクトは1000件までです".into()); }
    let allowed_types = ["point3d", "segment3d", "vector3d", "polygon3d", "plane3d", "cube", "cuboid", "prism", "pyramid", "cylinder", "cone", "sphere", "surface3d", "planarGraph3d", "sectionPlane", "label3d"];
    let mut ids = std::collections::HashSet::new();
    for (index, value) in objects.iter().enumerate() {
        let object = value.as_object().ok_or_else(|| format!("objects[{index}] が不正です"))?;
        reject_unknown_keys(object, &format!("objects[{index}]"), &["id", "type", "name", "visible", "locked", "transform", "style", "geometry", "labels", "metadata"])?;
        let id = object.get("id").and_then(Value::as_str).ok_or_else(|| format!("objects[{index}].id が必要です"))?;
        if !safe_graph_id(id) || !ids.insert(id.to_string()) { return Err(format!("objects[{index}].id が不正または重複しています")); }
        if object.get("type").and_then(Value::as_str).is_none_or(|value| !allowed_types.contains(&value)) { return Err(format!("objects[{index}].type が不正です")); }
        let kind = object.get("type").and_then(Value::as_str).unwrap();
        if object.get("name").and_then(Value::as_str).is_none_or(|value| value.chars().count() > 200) { return Err(format!("objects[{index}].name が不正です")); }
        if object.get("visible").and_then(Value::as_bool).is_none() || object.get("locked").and_then(Value::as_bool).is_none() { return Err(format!("objects[{index}] の表示・ロック設定が不正です")); }
        validate_spatial_value(object.get("name").unwrap_or(&Value::Null), &format!("objects[{index}].name"), 0)?;
        let transform = object.get("transform").and_then(Value::as_object).ok_or_else(|| format!("objects[{index}].transform が必要です"))?;
        reject_unknown_keys(transform, &format!("objects[{index}].transform"), &["position", "rotation", "scale"])?;
        finite_vec3(transform.get("position"), &format!("objects[{index}].transform.position"))?;
        finite_vec3(transform.get("rotation"), &format!("objects[{index}].transform.rotation"))?;
        finite_vec3(transform.get("scale"), &format!("objects[{index}].transform.scale"))?;
        let style = object.get("style").and_then(Value::as_object).ok_or_else(|| format!("objects[{index}].style が必要です"))?;
        reject_unknown_keys(style, &format!("objects[{index}].style"), &["lineColor", "lineWidth", "faceColor", "faceOpacity", "pointColor", "pointSize", "labelColor", "labelFontSize", "labelBackground", "hiddenLineColor", "hiddenLineWidth", "edgeOverrides"])?;
        for key in ["lineColor", "faceColor", "pointColor", "labelColor", "hiddenLineColor"] {
            if !is_hex_color(style.get(key)) { return Err(format!("objects[{index}].style.{key} が不正です")); }
        }
        bounded_number(style.get("lineWidth"), &format!("objects[{index}].style.lineWidth"), 0.25, 12.0)?;
        if style.contains_key("pointSize") { bounded_number(style.get("pointSize"), &format!("objects[{index}].style.pointSize"), 0.03, 1.0)?; }
        if style.contains_key("labelFontSize") { bounded_number(style.get("labelFontSize"), &format!("objects[{index}].style.labelFontSize"), 8.0, 72.0)?; }
        if style.get("labelBackground").is_some_and(|value| !matches!(value.as_str(), Some("transparent" | "white"))) { return Err(format!("objects[{index}].style.labelBackground が不正です")); }
        bounded_number(style.get("hiddenLineWidth"), &format!("objects[{index}].style.hiddenLineWidth"), 0.25, 12.0)?;
        bounded_number(style.get("faceOpacity"), &format!("objects[{index}].style.faceOpacity"), 0.0, 1.0)?;
        let overrides = style.get("edgeOverrides").and_then(Value::as_object).ok_or_else(|| format!("objects[{index}].style.edgeOverrides が必要です"))?;
        if overrides.len() > 2_000 || overrides.iter().any(|(key, value)| {
            let Some((left, right)) = key.split_once('-') else { return true; };
            left.parse::<usize>().is_err() || right.parse::<usize>().is_err() || !matches!(value.as_str(), Some("auto" | "solid" | "dashed" | "hidden"))
        }) { return Err(format!("objects[{index}].style.edgeOverrides が不正です")); }
        let geometry = object.get("geometry").and_then(Value::as_object).ok_or_else(|| format!("objects[{index}].geometry が必要です"))?;
        validate_spatial_geometry(kind, geometry, &format!("objects[{index}].geometry"))?;
        validate_spatial_value(object.get("geometry").unwrap(), &format!("objects[{index}].geometry"), 0)?;
        let labels = object.get("labels").and_then(Value::as_array).ok_or_else(|| format!("objects[{index}].labels が必要です"))?;
        if labels.len() > 200 { return Err(format!("objects[{index}].labels が多すぎます")); }
        for (label_index, label) in labels.iter().enumerate() {
            let label = label.as_object().ok_or_else(|| format!("objects[{index}].labels[{label_index}] が不正です"))?;
            reject_unknown_keys(label, &format!("objects[{index}].labels[{label_index}]"), &["id", "text", "position", "placement", "alwaysOnTop", "fontSize", "color", "background", "border"])?;
            if label.get("id").and_then(Value::as_str).is_none_or(|value| !safe_graph_id(value))
                || label.get("text").and_then(Value::as_str).is_none_or(|value| value.chars().count() > 1_000 || value.chars().any(char::is_control))
                || !matches!(label.get("placement").and_then(Value::as_str), Some("world" | "screen"))
                || label.get("alwaysOnTop").and_then(Value::as_bool).is_none()
                || label.get("background").and_then(Value::as_bool).is_none()
                || label.get("border").and_then(Value::as_bool).is_none()
                || !is_hex_color(label.get("color"))
            { return Err(format!("objects[{index}].labels[{label_index}] が不正です")); }
            finite_vec3(label.get("position"), &format!("objects[{index}].labels[{label_index}].position"))?;
            bounded_number(label.get("fontSize"), &format!("objects[{index}].labels[{label_index}].fontSize"), 6.0, 200.0)?;
        }
        validate_spatial_value(object.get("labels").unwrap_or(&Value::Null), &format!("objects[{index}].labels"), 0)?;
        if !object.get("metadata").is_some_and(Value::is_object) { return Err(format!("objects[{index}].metadata が不正です")); }
        validate_spatial_value(object.get("metadata").unwrap(), &format!("objects[{index}].metadata"), 0)?;
    }
    Ok(())
}

/// 既存アプリのProject形式を壊さず、サーバー境界でサイズと主要構造だけを厳格検証する。
fn validated_graph_json(text: &str) -> Result<String, String> {
    if text.is_empty() || text.len() > MAX_GRAPH_JSON {
        return Err(format!("graph.json は1〜{} bytesで指定してください", MAX_GRAPH_JSON));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| format!("graph.json が不正です: {e}"))?;
    let root = value
        .as_object()
        .ok_or_else(|| "graph.json のルートはオブジェクトである必要があります".to_string())?;
    if root.get("documentType").and_then(Value::as_str) == Some("spatial-geometry") {
        validate_spatial_graph(root)?;
        return serde_json::to_string_pretty(&value).map_err(err_str);
    }
    reject_unknown_keys(
        root,
        "graph.json",
        &["version", "appName", "expressions", "points", "labels", "range", "paper"],
    )?;
    if root.get("version").and_then(Value::as_i64) != Some(1) {
        return Err("未対応のgraph.json versionです".into());
    }
    if root.get("appName").and_then(Value::as_str) != Some("MathGraph PDF Studio") {
        return Err("graph.json のappNameが不正です".into());
    }
    let expressions = root
        .get("expressions")
        .and_then(Value::as_array)
        .ok_or_else(|| "graph.json にexpressions配列が必要です".to_string())?;
    if expressions.len() > 256 {
        return Err("式は256件までです".into());
    }
    for (index, expression) in expressions.iter().enumerate() {
        let object = expression
            .as_object()
            .ok_or_else(|| format!("expressions[{index}] が不正です"))?;
        reject_unknown_keys(
            object,
            &format!("expressions[{index}]"),
            &[
                "id", "input", "name", "visible", "color", "lineWidth", "lineStyle",
                "fillColor", "fillOpacity", "fillStyle", "tmin", "tmax",
            ],
        )?;
        let input = object.get("input").and_then(Value::as_str).unwrap_or("");
        if input.len() > 8_192 {
            return Err(format!("expressions[{index}].input が長すぎます"));
        }
    }
    let points = root
        .get("points")
        .and_then(Value::as_array)
        .ok_or_else(|| "graph.json にpoints配列が必要です".to_string())?;
    if points.len() > 1_000 {
        return Err("points は1000件までです".into());
    }
    for (index, point) in points.iter().enumerate() {
        let object = point
            .as_object()
            .ok_or_else(|| format!("points[{index}] が不正です"))?;
        reject_unknown_keys(
            object,
            &format!("points[{index}]"),
            &["id", "x", "y", "label", "color", "visible", "showProjectionToXAxis", "showProjectionToYAxis"],
        )?;
    }
    let labels = root
        .get("labels")
        .and_then(Value::as_array)
        .ok_or_else(|| "graph.json にlabels配列が必要です".to_string())?;
    if labels.len() > 1_000 {
        return Err("labels は1000件までです".into());
    }
    for (index, label) in labels.iter().enumerate() {
        let object = label
            .as_object()
            .ok_or_else(|| format!("labels[{index}] が不正です"))?;
        reject_unknown_keys(
            object,
            &format!("labels[{index}]"),
            &["id", "latex", "x", "y", "fontSize", "color", "visible"],
        )?;
    }
    let range = root
        .get("range")
        .and_then(Value::as_object)
        .ok_or_else(|| "graph.json にrangeが必要です".to_string())?;
    reject_unknown_keys(range, "range", &["xmin", "xmax", "ymin", "ymax", "xstep", "ystep"])?;
    let xmin = finite_number(range, "xmin")?;
    let xmax = finite_number(range, "xmax")?;
    let ymin = finite_number(range, "ymin")?;
    let ymax = finite_number(range, "ymax")?;
    let xstep = finite_number(range, "xstep")?;
    let ystep = finite_number(range, "ystep")?;
    if xmin >= xmax || ymin >= ymax || xstep <= 0.0 || ystep <= 0.0 {
        return Err("表示範囲または目盛り間隔が不正です".into());
    }
    if xmax - xmin > 1.0e9 || ymax - ymin > 1.0e9 {
        return Err("表示範囲が大きすぎます".into());
    }
    let paper = root
        .get("paper")
        .and_then(Value::as_object)
        .ok_or_else(|| "graph.json にpaper設定が必要です".to_string())?;
    // 古い保存データは不足項目をクライアントが既定値で補うため、requiredにはしない。
    // 一方、サーバー境界では未知キーを拒否し、コマンド名やパス等の混入を防ぐ。
    reject_unknown_keys(
        paper,
        "paper",
        &[
            "orientation", "title", "subtitle", "problemNumber", "caption", "showAxes",
            "axisLabelX", "axisLabelY", "axisLabelO", "axisLabelSize", "showTicks", "showGrid",
            "showLegend", "legendFontSize", "showIntersections", "showIntersectionCoords",
            "regionMode", "intersectionColor", "intersectionOpacity", "intersectionStyle",
            "lockAspect", "aspectMode", "customAspectRatio", "marginMm", "pdfGraphOnly",
            "pdfGraphWidthMm", "pdfAspectMode", "pdfCustomAspectRatio",
        ],
    )?;
    serde_json::to_string_pretty(&value).map_err(err_str)
}

fn warnings_json(warnings: Option<Vec<String>>) -> String {
    let clean: Vec<String> = warnings
        .unwrap_or_default()
        .into_iter()
        .take(100)
        .map(|w| w.chars().take(500).collect())
        .collect();
    serde_json::to_string(&clean).unwrap_or_else(|_| "[]".into())
}

fn formats_in_dir(dir: &Path) -> Vec<String> {
    ["pdf", "png", "svg", "tex", "json"]
        .into_iter()
        .filter(|ext| dir.join(format!("graph.{ext}")).is_file())
        .map(str::to_string)
        .collect()
}

fn row_summary(state: &AppState, row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphSummary> {
    let id: String = row.get(0)?;
    let warnings_text: String = row.get(4)?;
    let dir = state.graph_dir(&id);
    let exports_current = match (
        fs::metadata(dir.join("graph.json")).and_then(|value| value.modified()),
        fs::metadata(dir.join("graph.pdf")).and_then(|value| value.modified()),
    ) {
        (Ok(source), Ok(pdf)) => pdf >= source,
        _ => false,
    };
    let has_thumbnail = dir.join("thumbnail.png").is_file() || dir.join("graph.png").is_file();
    let thumbnail_path = if has_thumbnail {
        format!("/api/graphs/{id}/files/thumbnail")
    } else {
        String::new()
    };
    Ok(GraphSummary {
        saved_formats: formats_in_dir(&state.graph_dir(&id)),
        exports_current,
        id,
        title: row.get(1)?,
        graph_type: row.get(2)?,
        source_type: row.get(3)?,
        warnings: serde_json::from_str(&warnings_text).unwrap_or_default(),
        // WebへAppDataの絶対パスを送らない。認証済みの安定URLだけを返す。
        thumbnail_path,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        version: row.get(8)?,
        usage_count: row.get(9)?,
    })
}

pub fn list_graph_versions(state: &AppState, graph_id: String) -> Result<Vec<GraphVersionSummary>, String> {
    if !safe_graph_id(&graph_id) {
        return Err("不正なグラフIDです".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT id,graph_id,title,version,saved_at FROM graph_versions
             WHERE graph_id=?1 ORDER BY version DESC,id DESC",
        )
        .map_err(err_str)?;
    let rows = stmt.query_map(params![graph_id], |row| {
        Ok(GraphVersionSummary {
            id: row.get(0)?,
            graph_id: row.get(1)?,
            title: row.get(2)?,
            version: row.get(3)?,
            saved_at: row.get(4)?,
        })
    })
    .map_err(err_str)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(err_str)?;
    Ok(rows)
}

pub fn get_graph_version(state: &AppState, version_id: i64) -> Result<GraphVersionFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.query_row(
        "SELECT id,graph_id,title,version,saved_at,graph_json,graph_type,source_type,warnings_json
         FROM graph_versions WHERE id=?1",
        params![version_id],
        |row| {
            let warnings_text: String = row.get(8)?;
            Ok(GraphVersionFull {
                summary: GraphVersionSummary {
                    id: row.get(0)?,
                    graph_id: row.get(1)?,
                    title: row.get(2)?,
                    version: row.get(3)?,
                    saved_at: row.get(4)?,
                },
                graph_json: row.get(5)?,
                graph_type: row.get(6)?,
                source_type: row.get(7)?,
                warnings: serde_json::from_str(&warnings_text).unwrap_or_default(),
            })
        },
    )
    .optional()
    .map_err(err_str)?
    .ok_or_else(|| "グラフ履歴が見つかりません".into())
}

pub fn list_graphs(state: &AppState, include_deleted: Option<bool>) -> Result<Vec<GraphSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let where_clause = if include_deleted.unwrap_or(false) {
        ""
    } else {
        "WHERE g.deleted_at=''"
    };
    let sql = format!(
        "SELECT g.id, g.title, g.graph_type, g.source_type, g.warnings_json,
                g.thumbnail_path, g.created_at, g.updated_at, g.version,
                (SELECT COUNT(*) FROM graph_assets a WHERE a.graph_id=g.id)
         FROM graphs g {where_clause} ORDER BY g.updated_at DESC"
    );
    let mut stmt = conn.prepare(&sql).map_err(err_str)?;
    let rows = stmt
        .query_map([], |row| row_summary(state, row))
        .map_err(err_str)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn get_graph(state: &AppState, id: String) -> Result<GraphFull, String> {
    if !safe_graph_id(&id) {
        return Err("不正なグラフIDです".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    conn.query_row(
        "SELECT g.id, g.title, g.graph_type, g.source_type, g.warnings_json,
                g.thumbnail_path, g.created_at, g.updated_at, g.version,
                (SELECT COUNT(*) FROM graph_assets a WHERE a.graph_id=g.id), g.graph_json
         FROM graphs g WHERE g.id=?1 AND g.deleted_at=''",
        params![id],
        |row| {
            let summary = row_summary(state, row)?;
            Ok(GraphFull { summary, graph_json: row.get(10)? })
        },
    )
    .optional()
    .map_err(err_str)?
    .ok_or_else(|| "グラフが見つかりません".into())
}

/// 旧Windows連携で保存済みのgraph assetを、Web編集用の共通正本へ安全に取り込む。
/// DB内のパスであってもgraph_assets配下に実在するgraph.jsonだけを許可する。
pub fn ensure_graph_from_asset(state: &AppState, asset_id: String) -> Result<String, String> {
    if !safe_graph_id(&asset_id) {
        return Err("不正なグラフasset IDです".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    let (stored_graph_id, display_name, editable_source_path): (String, String, String) = conn
        .query_row(
            "SELECT graph_id,display_name,editable_source_path FROM graph_assets WHERE asset_id=?1",
            params![asset_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフassetが見つかりません".to_string())?;
    if safe_graph_id(&stored_graph_id) {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM graphs WHERE id=?1 AND deleted_at='')",
                params![stored_graph_id],
                |row| row.get(0),
            )
            .map_err(err_str)?;
        if exists {
            return Ok(stored_graph_id);
        }
    }
    let root = fs::canonicalize(state.graph_assets_dir()).map_err(err_str)?;
    let source = fs::canonicalize(&editable_source_path)
        .map_err(|_| "編集用graph.jsonが見つかりません".to_string())?;
    if !source.starts_with(&root) || !source.is_file() {
        return Err("編集用graph.jsonの保存場所が不正です".into());
    }
    let metadata = fs::metadata(&source).map_err(err_str)?;
    if metadata.len() == 0 || metadata.len() > MAX_GRAPH_JSON as u64 {
        return Err("編集用graph.jsonのサイズが不正です".into());
    }
    let graph_json = validated_graph_json(&fs::read_to_string(&source).map_err(err_str)?)?;
    let graph_id = if safe_graph_id(&stored_graph_id) {
        stored_graph_id
    } else {
        format!("graph_{}", uuid::Uuid::new_v4().simple())
    };
    let title = clean_title(&display_name)?;
    write_graph_json(state, &graph_id, &graph_json)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO graphs (id,title,graph_json,graph_type,source_type,warnings_json,created_at,updated_at)
         VALUES (?1,?2,?3,'function_graph','import','[]',?4,?4)",
        params![graph_id, title, graph_json, now],
    )
    .map_err(err_str)?;
    conn.execute(
        "UPDATE graph_assets SET graph_id=?1 WHERE asset_id=?2",
        params![graph_id, asset_id],
    )
    .map_err(err_str)?;
    Ok(graph_id)
}

fn write_graph_json(state: &AppState, id: &str, graph_json: &str) -> Result<PathBuf, String> {
    let dir = state.graph_dir(id);
    fs::create_dir_all(&dir).map_err(err_str)?;
    let temp = dir.join(format!("graph.json.tmp-{}", uuid::Uuid::new_v4().simple()));
    fs::write(&temp, graph_json).map_err(err_str)?;
    let dest = dir.join("graph.json");
    if dest.exists() {
        fs::remove_file(&dest).map_err(err_str)?;
    }
    fs::rename(&temp, &dest).map_err(err_str)?;
    Ok(dest)
}

pub fn create_graph(state: &AppState, payload: CreateGraphPayload) -> Result<String, String> {
    let title = clean_title(&payload.title)?;
    let graph_json = validated_graph_json(&payload.graph_json)?;
    let graph_type = safe_enum(payload.graph_type, &["function_graph", "geometry", "mixed", "spatial_geometry"], "function_graph");
    let source_type = safe_enum(payload.source_type, &["manual", "ai_text", "ai_image", "ai_problem", "import"], "manual");
    let warnings = warnings_json(payload.warnings);
    let id = format!("graph_{}", uuid::Uuid::new_v4().simple());
    write_graph_json(state, &id, &graph_json)?;
    let now = now_str();
    let conn = state.conn.lock().map_err(err_str)?;
    if let Err(error) = conn.execute(
        "INSERT INTO graphs (id,title,graph_json,graph_type,source_type,warnings_json,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
        params![id, title, graph_json, graph_type, source_type, warnings, now],
    ) {
        fs::remove_dir_all(state.graph_dir(&id)).ok();
        return Err(error.to_string());
    }
    Ok(id)
}

pub fn update_graph(state: &AppState, payload: SaveGraphPayload) -> Result<i64, String> {
    if !safe_graph_id(&payload.id) {
        return Err("不正なグラフIDです".into());
    }
    let title = clean_title(&payload.title)?;
    let graph_json = validated_graph_json(&payload.graph_json)?;
    let mut conn = state.conn.lock().map_err(err_str)?;
    let current = conn
        .query_row(
            "SELECT title,graph_json,graph_type,source_type,warnings_json,version FROM graphs WHERE id=?1 AND deleted_at=''",
            params![payload.id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?)),
        )
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフが見つかりません".to_string())?;
    if payload.expected_version.is_some_and(|expected| expected != current.5) {
        return Err(format!("CONFLICT:{}", current.5));
    }
    let graph_type = safe_enum(payload.graph_type, &["function_graph", "geometry", "mixed", "spatial_geometry"], &current.2);
    let source_type = safe_enum(payload.source_type, &["manual", "ai_text", "ai_image", "ai_problem", "import"], &current.3);
    let warnings = payload.warnings.map(|v| warnings_json(Some(v))).unwrap_or(current.4.clone());
    write_graph_json(state, &payload.id, &graph_json)?;
    let tx = conn.transaction().map_err(err_str)?;
    tx.execute(
        "INSERT INTO graph_versions (graph_id,title,graph_json,graph_type,source_type,warnings_json,version,saved_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![payload.id, current.0, current.1, current.2, current.3, current.4, current.5, now_str()],
    )
    .map_err(err_str)?;
    tx.execute(
        "UPDATE graphs SET title=?1,graph_json=?2,graph_type=?3,source_type=?4,warnings_json=?5,
                updated_at=?6,version=version+1 WHERE id=?7",
        params![title, graph_json, graph_type, source_type, warnings, now_str(), payload.id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    // graph.jsonと一致しない古い派生出力は公開しない。次回exportで再生成される。
    for name in ["graph.pdf", "graph.png", "graph.svg", "graph.tex", "graph.zip", "thumbnail.png"] {
        fs::remove_file(state.graph_dir(&payload.id).join(name)).ok();
    }
    conn.execute("UPDATE graphs SET thumbnail_path='' WHERE id=?1", params![payload.id])
        .map_err(err_str)?;
    Ok(current.5 + 1)
}

pub fn restore_graph_version(
    state: &AppState,
    version_id: i64,
    expected_version: Option<i64>,
) -> Result<i64, String> {
    let historical = get_graph_version(state, version_id)?;
    let graph_json = validated_graph_json(&historical.graph_json)?;
    let graph_id = historical.summary.graph_id.clone();
    let mut conn = state.conn.lock().map_err(err_str)?;
    let current = conn
        .query_row(
            "SELECT title,graph_json,graph_type,source_type,warnings_json,version
             FROM graphs WHERE id=?1 AND deleted_at=''",
            params![graph_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?)),
        )
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフが見つかりません".to_string())?;
    if expected_version.is_some_and(|expected| expected != current.5) {
        return Err(format!("CONFLICT:{}", current.5));
    }
    write_graph_json(state, &graph_id, &graph_json)?;
    let next_version = current.5 + 1;
    let tx = conn.transaction().map_err(err_str)?;
    tx.execute(
        "INSERT INTO graph_versions (graph_id,title,graph_json,graph_type,source_type,warnings_json,version,saved_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![graph_id, current.0, current.1, current.2, current.3, current.4, current.5, now_str()],
    )
    .map_err(err_str)?;
    tx.execute(
        "UPDATE graphs SET title=?1,graph_json=?2,graph_type=?3,source_type=?4,warnings_json=?5,
                thumbnail_path='',updated_at=?6,version=?7 WHERE id=?8",
        params![
            historical.summary.title,
            graph_json,
            historical.graph_type,
            historical.source_type,
            serde_json::to_string(&historical.warnings).unwrap_or_else(|_| "[]".into()),
            now_str(),
            next_version,
            graph_id,
        ],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    for name in ["graph.pdf", "graph.png", "graph.svg", "graph.tex", "graph.zip", "thumbnail.png"] {
        fs::remove_file(state.graph_dir(&historical.summary.graph_id).join(name)).ok();
    }
    Ok(next_version)
}

pub fn duplicate_graph(state: &AppState, id: String) -> Result<String, String> {
    let graph = get_graph(state, id)?;
    create_graph(
        state,
        CreateGraphPayload {
            title: format!("{} (コピー)", graph.summary.title),
            graph_json: graph.graph_json,
            graph_type: Some(graph.summary.graph_type),
            source_type: Some("manual".into()),
            warnings: Some(graph.summary.warnings),
        },
    )
}

pub fn delete_graph(state: &AppState, id: String, expected_version: Option<i64>) -> Result<(), String> {
    if !safe_graph_id(&id) {
        return Err("不正なグラフIDです".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    let current: i64 = conn
        .query_row("SELECT version FROM graphs WHERE id=?1 AND deleted_at=''", params![id], |r| r.get(0))
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフが見つかりません".to_string())?;
    if expected_version.is_some_and(|v| v != current) {
        return Err(format!("CONFLICT:{current}"));
    }
    conn.execute(
        "UPDATE graphs SET deleted_at=?1,updated_at=?1,version=version+1 WHERE id=?2",
        params![now_str(), id],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn restore_graph(state: &AppState, id: String) -> Result<(), String> {
    if !safe_graph_id(&id) {
        return Err("不正なグラフIDです".into());
    }
    let conn = state.conn.lock().map_err(err_str)?;
    let changed = conn
        .execute(
            "UPDATE graphs SET deleted_at='',updated_at=?1,version=version+1 WHERE id=?2 AND deleted_at<>''",
            params![now_str(), id],
        )
        .map_err(err_str)?;
    if changed == 0 {
        return Err("復元できるグラフが見つかりません".into());
    }
    Ok(())
}

fn validate_export(name: &str, bytes: &[u8]) -> Result<(), String> {
    match name {
        "pdf" if bytes.starts_with(b"%PDF-") => Ok(()),
        "png" if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) => Ok(()),
        "svg" => {
            let text = std::str::from_utf8(bytes).map_err(|_| "SVGはUTF-8である必要があります")?;
            let lower = text.to_ascii_lowercase();
            let mut root = lower.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
            if root.starts_with("<?xml") {
                let end = root.find("?>").ok_or_else(|| "SVGのXML宣言が不正です".to_string())?;
                root = root[end + 2..].trim_start();
            }
            if !root.starts_with("<svg")
                || ["<script", "javascript:", "<foreignobject", "onload=", "onerror="]
                    .iter()
                    .any(|bad| lower.contains(bad))
            {
                return Err("SVGに危険または不正な要素があります".into());
            }
            Ok(())
        }
        "tex" => {
            let text = std::str::from_utf8(bytes).map_err(|_| "TikZはUTF-8である必要があります")?;
            let lower = text.to_ascii_lowercase();
            if ["\\write18", "\\input|", "\\immediate\\write", "\\openout", "\\includegraphics{/", "\\includegraphics{\\\\"]
                .iter()
                .any(|bad| lower.contains(bad))
            {
                return Err("TikZに危険な命令があります".into());
            }
            Ok(())
        }
        _ => Err(format!("{} の実ファイル形式が一致しません", name)),
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    crc ^ 0xffff_ffff
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

/// 依存crateを増やさず、ブラウザへstream可能なZIP（store方式）をPC側で組み立てる。
fn build_store_zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if files.is_empty() || files.len() > u16::MAX as usize {
        return Err("ZIPへまとめるファイル数が不正です".into());
    }
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, bytes) in files {
        let name_bytes = name.as_bytes();
        let name_len = u16::try_from(name_bytes.len()).map_err(|_| "ZIP内ファイル名が長すぎます")?;
        let size = u32::try_from(bytes.len()).map_err(|_| "ZIP内ファイルが大きすぎます")?;
        let offset = u32::try_from(local.len()).map_err(|_| "ZIPが大きすぎます")?;
        let checksum = crc32(bytes);

        push_u32(&mut local, 0x0403_4b50);
        push_u16(&mut local, 20);
        push_u16(&mut local, 0);
        push_u16(&mut local, 0);
        push_u16(&mut local, 0);
        push_u16(&mut local, 0);
        push_u32(&mut local, checksum);
        push_u32(&mut local, size);
        push_u32(&mut local, size);
        push_u16(&mut local, name_len);
        push_u16(&mut local, 0);
        local.extend_from_slice(name_bytes);
        local.extend_from_slice(bytes);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, checksum);
        push_u32(&mut central, size);
        push_u32(&mut central, size);
        push_u16(&mut central, name_len);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, offset);
        central.extend_from_slice(name_bytes);
    }
    let central_offset = u32::try_from(local.len()).map_err(|_| "ZIPが大きすぎます")?;
    let central_size = u32::try_from(central.len()).map_err(|_| "ZIPが大きすぎます")?;
    let count = files.len() as u16;
    local.extend_from_slice(&central);
    push_u32(&mut local, 0x0605_4b50);
    push_u16(&mut local, 0);
    push_u16(&mut local, 0);
    push_u16(&mut local, count);
    push_u16(&mut local, count);
    push_u32(&mut local, central_size);
    push_u32(&mut local, central_offset);
    push_u16(&mut local, 0);
    Ok(local)
}

fn rebuild_graph_zip(dir: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for name in ["graph.pdf", "graph.png", "graph.svg", "graph.tex", "graph.json"] {
        let path = dir.join(name);
        if path.is_file() {
            files.push((name.to_string(), fs::read(path).map_err(err_str)?));
        }
    }
    let zip = build_store_zip(&files)?;
    let temp = dir.join(format!(".graph.zip.{}", uuid::Uuid::new_v4().simple()));
    fs::write(&temp, zip).map_err(err_str)?;
    let destination = dir.join("graph.zip");
    if destination.exists() {
        fs::remove_file(&destination).map_err(err_str)?;
    }
    fs::rename(temp, destination).map_err(err_str)
}

/// 認証済みHTTP配信で使用するグラフファイルを、IDと列挙formatだけから解決する。
/// ブラウザからローカルパスを受け取らない。
pub fn graph_file_path(state: &AppState, id: &str, format: &str) -> Result<PathBuf, String> {
    if !safe_graph_id(id) {
        return Err("不正なグラフIDです".into());
    }
    let exists = {
        let conn = state.conn.lock().map_err(err_str)?;
        conn.query_row(
            "SELECT 1 FROM graphs WHERE id=?1 AND deleted_at=''",
            params![id],
            |_| Ok(true),
        )
        .optional()
        .map_err(err_str)?
        .unwrap_or(false)
    };
    if !exists {
        return Err("グラフが見つかりません".into());
    }
    let dir = state.graph_dir(id);
    let path = match format {
        "pdf" => dir.join("graph.pdf"),
        "png" => dir.join("graph.png"),
        "svg" => dir.join("graph.svg"),
        "tex" => dir.join("graph.tex"),
        "json" => dir.join("graph.json"),
        "zip" => dir.join("graph.zip"),
        "thumbnail" if dir.join("thumbnail.png").is_file() => dir.join("thumbnail.png"),
        "thumbnail" => dir.join("graph.png"),
        _ => return Err("未対応のグラフファイル形式です".into()),
    };
    if !path.is_file() {
        return Err("グラフファイルが見つかりません".into());
    }
    Ok(path)
}

pub fn save_graph_exports(
    state: &AppState,
    id: String,
    files: BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    if !safe_graph_id(&id) {
        return Err("不正なグラフIDです".into());
    }
    {
        let conn = state.conn.lock().map_err(err_str)?;
        let exists: bool = conn
            .query_row("SELECT 1 FROM graphs WHERE id=?1 AND deleted_at=''", params![id], |_| Ok(true))
            .optional()
            .map_err(err_str)?
            .unwrap_or(false);
        if !exists {
            return Err("グラフが見つかりません".into());
        }
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let mut decoded = Vec::new();
    let mut total = 0usize;
    for (name, encoded) in files {
        if !["pdf", "png", "svg", "tex"].contains(&name.as_str()) {
            return Err(format!("未対応の出力形式です: {name}"));
        }
        let bytes = engine.decode(encoded).map_err(|_| format!("{name} のBase64が不正です"))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_EXPORT_TOTAL {
            return Err("出力ファイルの合計サイズが上限を超えています".into());
        }
        validate_export(&name, &bytes)?;
        decoded.push((name, bytes));
    }
    let dir = state.graph_dir(&id);
    fs::create_dir_all(&dir).map_err(err_str)?;
    let mut saved = Vec::new();
    let mut thumbnail = None;
    for (name, bytes) in decoded {
        let dest = dir.join(format!("graph.{name}"));
        let temp = dir.join(format!(".graph.{name}.{}", uuid::Uuid::new_v4().simple()));
        fs::write(&temp, bytes).map_err(err_str)?;
        if dest.exists() {
            fs::remove_file(&dest).map_err(err_str)?;
        }
        fs::rename(temp, &dest).map_err(err_str)?;
        if name == "png" {
            thumbnail = Some(dest.to_string_lossy().to_string());
        }
        saved.push(name);
    }
    rebuild_graph_zip(&dir)?;
    if let Some(path) = thumbnail {
        let conn = state.conn.lock().map_err(err_str)?;
        conn.execute("UPDATE graphs SET thumbnail_path=?1 WHERE id=?2", params![path, id])
            .map_err(err_str)?;
    }
    Ok(saved)
}

#[derive(Clone, Copy)]
pub struct GraphAssetTarget {
    pub project_id: Option<i64>,
    pub problem_id: Option<i64>,
    pub item_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSnapshotResult {
    pub asset_id: String,
    pub inserted_latex: String,
}

pub(crate) struct PreparedGraphSnapshot {
    pub(crate) result: GraphSnapshotResult,
    pub(crate) graph_id: String,
    pub(crate) title: String,
    pub(crate) graph_version: i64,
    pub(crate) snapshot_dir: PathBuf,
}

pub(crate) fn prepare_graph_snapshot(
    state: &AppState,
    conn: &rusqlite::Connection,
    graph_id: &str,
) -> Result<PreparedGraphSnapshot, String> {
    if !safe_graph_id(graph_id) {
        return Err("不正なグラフIDです".into());
    }
    let dir = state.graph_dir(graph_id);
    if !dir.join("graph.pdf").is_file() || !dir.join("graph.json").is_file() {
        return Err("教材へ挿入する前にPDFとgraph.jsonを保存してください".into());
    }
    let source_modified = fs::metadata(dir.join("graph.json")).and_then(|value| value.modified()).map_err(err_str)?;
    let pdf_modified = fs::metadata(dir.join("graph.pdf")).and_then(|value| value.modified()).map_err(err_str)?;
    if pdf_modified < source_modified {
        return Err("グラフが更新されています。開いて最新のPDFを生成してから挿入してください".into());
    }
    let (title, graph_version): (String, i64) = conn
        .query_row(
            "SELECT title,version FROM graphs WHERE id=?1 AND deleted_at=''",
            params![graph_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフが見つかりません".to_string())?;
    let asset_id = format!("graphasset_{}", uuid::Uuid::new_v4().simple());
    let snapshot_dir = state.graph_assets_dir().join("snapshots").join(&asset_id);
    fs::create_dir_all(&snapshot_dir).map_err(err_str)?;
    let copy_result = (|| -> Result<(), String> {
        for name in ["graph.json", "graph.pdf", "graph.png", "graph.svg", "graph.tex", "thumbnail.png"] {
            let source = dir.join(name);
            if source.is_file() {
                fs::copy(&source, snapshot_dir.join(name)).map_err(err_str)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = copy_result {
        fs::remove_dir_all(&snapshot_dir).ok();
        return Err(error);
    }
    let insert_width = safe_width(get_setting(conn, "graph_insert_width").as_deref());
    let inserted_latex = format!(
        "\\begin{{center}}\n  \\includegraphics[width={},height=0.72\\textheight,keepaspectratio]{{assets/graphs/snapshots/{}/graph.pdf}}\n\\end{{center}}",
        insert_width, asset_id
    );
    Ok(PreparedGraphSnapshot {
        result: GraphSnapshotResult { asset_id, inserted_latex },
        graph_id: graph_id.to_string(),
        title,
        graph_version,
        snapshot_dir,
    })
}

pub(crate) fn register_graph_snapshot(
    conn: &rusqlite::Connection,
    prepared: &PreparedGraphSnapshot,
    target: GraphAssetTarget,
) -> Result<(), String> {
    let now = now_str();
    conn.execute(
        "INSERT INTO graph_assets (asset_id,graph_id,display_name,project_id,problem_id,item_id,editable_source_path,
                primary_asset_path,preview_asset_path,latex_source_path,inserted_latex,metadata_json,
                created_at,updated_at,version)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13,?14)",
        params![
            prepared.result.asset_id,
            prepared.graph_id,
            prepared.title,
            target.project_id,
            target.problem_id,
            target.item_id,
            prepared.snapshot_dir.join("graph.json").to_string_lossy(),
            prepared.snapshot_dir.join("graph.pdf").to_string_lossy(),
            if prepared.snapshot_dir.join("graph.png").is_file() { prepared.snapshot_dir.join("graph.png").to_string_lossy().to_string() } else { String::new() },
            if prepared.snapshot_dir.join("graph.tex").is_file() { prepared.snapshot_dir.join("graph.tex").to_string_lossy().to_string() } else { String::new() },
            prepared.result.inserted_latex,
            serde_json::json!({"snapshot":true,"graphVersion":prepared.graph_version}).to_string(),
            now,
            prepared.graph_version,
        ],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn snapshot_graph_asset(
    state: &AppState,
    graph_id: String,
    target: GraphAssetTarget,
) -> Result<GraphSnapshotResult, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let prepared = prepare_graph_snapshot(state, &conn, &graph_id)?;
    let result = (|| -> Result<(), String> {
        let tx = conn.transaction().map_err(err_str)?;
        register_graph_snapshot(&tx, &prepared, target)?;
        tx.commit().map_err(err_str)
    })();
    if let Err(error) = result {
        fs::remove_dir_all(&prepared.snapshot_dir).ok();
        return Err(error);
    }
    Ok(prepared.result)
}

pub fn insert_graph_to_project(
    state: &AppState,
    id: String,
    project_id: i64,
    expected_project_version: Option<i64>,
) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let current_project_version: i64 = conn
        .query_row("SELECT version FROM projects WHERE id=?1", params![project_id], |row| row.get(0))
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "教材プロジェクトが見つかりません".to_string())?;
    if expected_project_version.is_some_and(|expected| expected != current_project_version) {
        return Err(format!("CONFLICT:{current_project_version}"));
    }
    let prepared = prepare_graph_snapshot(state, &conn, &id)?;
    let insert_result = (|| -> Result<i64, String> {
        let tx = conn.transaction().map_err(err_str)?;
        let checked_project_version: i64 = tx
            .query_row("SELECT version FROM projects WHERE id=?1", params![project_id], |row| row.get(0))
            .map_err(err_str)?;
        if checked_project_version != current_project_version {
            return Err(format!("CONFLICT:{checked_project_version}"));
        }
        let order: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sort_order),-1)+1 FROM project_items WHERE project_id=?1",
            params![project_id],
            |row| row.get(0),
        ).map_err(err_str)?;
        tx.execute(
            "INSERT INTO project_items (project_id,item_type,sort_order,content,created_at) VALUES (?1,'text',?2,?3,?4)",
            params![project_id, order, prepared.result.inserted_latex, now_str()],
        ).map_err(err_str)?;
        let item_id = tx.last_insert_rowid();
        register_graph_snapshot(&tx, &prepared, GraphAssetTarget {
            project_id: Some(project_id), problem_id: None, item_id: Some(item_id),
        })?;
        let changed = tx.execute(
            "UPDATE projects SET updated_at=?1,version=version+1 WHERE id=?2 AND version=?3",
            params![now_str(), project_id, current_project_version],
        ).map_err(err_str)?;
        if changed == 0 {
            return Err(format!("CONFLICT:{checked_project_version}"));
        }
        tx.commit().map_err(err_str)?;
        Ok(item_id)
    })();
    if insert_result.is_err() {
        fs::remove_dir_all(&prepared.snapshot_dir).ok();
    }
    insert_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json() -> String {
        serde_json::json!({
            "version": 1,
            "appName": "MathGraph PDF Studio",
            "expressions": [],
            "points": [],
            "labels": [],
            "range": {"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},
            "paper": {}
        })
        .to_string()
    }

    fn valid_spatial_json() -> String {
        serde_json::json!({
            "schemaVersion": 2,
            "documentType": "spatial-geometry",
            "id": "document_test",
            "title": "立方体ABCD-EFGH",
            "projection": {"type":"orthographic","cameraPosition":[6,5,7],"target":[0,0,0],"up":[0,1,0],"zoom":1,"fov":38,"viewHeight":12,"preset":"textbook"},
            "output": {"widthMm":160,"heightMm":110,"pixelWidth":1600},
            "scene": {"background":"white","showAxes":false,"axesColor":"#334155","axesLabelSize":16,"axesLabelGap":8,"axesLabels":{"x":"x","y":"y","z":"z"},"axesLabelBackground":"transparent","showOriginLabel":true,"originLabel":"O","originLabelPosition":[-0.3,-0.3,0],"showGrid":false,"showHiddenEdges":true,"quality":"standard"},
            "objects": [{
                "id":"object_cube","type":"cube","name":"立方体","visible":true,"locked":false,
                "transform":{"position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]},
                "style":{"lineColor":"#172033","lineWidth":2,"faceColor":"#dbeafe","faceOpacity":0.2,"pointColor":"#dc2626","pointSize":0.16,"labelColor":"#111827","labelFontSize":18,"labelBackground":"transparent","hiddenLineColor":"#64748b","hiddenLineWidth":1.35,"edgeOverrides":{}},
                "geometry":{"sideLength":4,"vertexNames":["A","B","C","D","E","F","G","H"]},"labels":[],"metadata":{}
            }],
            "createdAt":"2026-07-13T00:00:00Z","updatedAt":"2026-07-13T00:00:00Z","version":1
        }).to_string()
    }

    #[test]
    fn graph_json_validation_rejects_wrong_app_and_range() {
        assert!(validated_graph_json(&valid_json()).is_ok());
        let wrong = valid_json().replace("MathGraph PDF Studio", "Other");
        assert!(validated_graph_json(&wrong).is_err());
        let wrong_range = valid_json().replace("\"xmax\":5", "\"xmax\":-5");
        assert!(validated_graph_json(&wrong_range).is_err());
        let unknown = valid_json().replace("\"paper\":{}", "\"paper\":{\"command\":\"calc.exe\"}");
        assert!(validated_graph_json(&unknown).is_err());
    }

    #[test]
    fn spatial_graph_validation_accepts_document_and_rejects_unsafe_values() {
        assert!(validated_graph_json(&valid_spatial_json()).is_ok());
        let huge = valid_spatial_json().replace("[0,0,0]", "[1000001,0,0]");
        assert!(validated_graph_json(&huge).is_err());
        let command = valid_spatial_json().replace("立方体", "cmd.exe /c calc");
        assert!(validated_graph_json(&command).is_err());
        let unknown = valid_spatial_json().replace("\"metadata\":{}", "\"metadata\":{},\"command\":\"calc\"");
        assert!(validated_graph_json(&unknown).is_err());
        let bad_geometry = valid_spatial_json().replace("\"sideLength\":4", "\"sideLength\":\"four\"");
        assert!(validated_graph_json(&bad_geometry).is_err());
        let bad_scene = valid_spatial_json().replace("\"showAxes\":false", "\"showAxes\":\"false\"");
        assert!(validated_graph_json(&bad_scene).is_err());
        let mut surface: Value = serde_json::from_str(&valid_spatial_json()).unwrap();
        surface["objects"][0]["type"] = serde_json::json!("surface3d");
        surface["objects"][0]["name"] = serde_json::json!("放物面");
        surface["objects"][0]["geometry"] = serde_json::json!({"expression":"z = x^2 + y^2","xMin":-3,"xMax":3,"yMin":-3,"yMax":3,"resolution":28,"wireframe":true});
        assert!(validated_graph_json(&surface.to_string()).is_ok());
        surface["objects"][0]["geometry"]["expression"] = serde_json::json!("powershell http://evil.invalid");
        assert!(validated_graph_json(&surface.to_string()).is_err());
        let mut planar: Value = serde_json::from_str(&valid_spatial_json()).unwrap();
        planar["objects"][0]["type"] = serde_json::json!("planarGraph3d");
        planar["objects"][0]["name"] = serde_json::json!("XY平面の領域");
        planar["objects"][0]["geometry"] = serde_json::json!({"expression":"x^2+y^2<=4 ^ 1<x<3/2","xMin":-4,"xMax":4,"yMin":-4,"yMax":4,"resolution":64,"tMin":0,"tMax":6.283185307179586,"fill":true,"plane":"xy"});
        assert!(validated_graph_json(&planar.to_string()).is_ok());
        planar["objects"][0]["geometry"]["resolution"] = serde_json::json!(500);
        assert!(validated_graph_json(&planar.to_string()).is_err());
        planar["objects"][0]["geometry"]["resolution"] = serde_json::json!(64);
        planar["objects"][0]["geometry"]["plane"] = serde_json::json!("ab");
        assert!(validated_graph_json(&planar.to_string()).is_err());
        planar["objects"][0]["geometry"]["plane"] = serde_json::json!("xy");
        planar["objects"][0]["geometry"]["xMin"] = serde_json::json!(-200_000);
        planar["objects"][0]["geometry"]["xMax"] = serde_json::json!(200_000);
        planar["objects"][0]["geometry"]["yMin"] = serde_json::json!(-200_000);
        planar["objects"][0]["geometry"]["yMax"] = serde_json::json!(200_000);
        assert!(validated_graph_json(&planar.to_string()).is_ok());
        let bad_axis_label = valid_spatial_json().replace("\"axesLabelSize\":16", "\"axesLabelSize\":1000");
        assert!(validated_graph_json(&bad_axis_label).is_err());
        let bad_axis_gap = valid_spatial_json().replace("\"axesLabelGap\":8", "\"axesLabelGap\":1000");
        assert!(validated_graph_json(&bad_axis_gap).is_err());
        let bad_axis_text = valid_spatial_json().replace("\"x\":\"x\"", "\"x\":7");
        assert!(validated_graph_json(&bad_axis_text).is_err());
        let bad_origin_position = valid_spatial_json().replace("\"originLabelPosition\":[-0.3,-0.3,0]", "\"originLabelPosition\":[-2000000,-0.3,0]");
        assert!(validated_graph_json(&bad_origin_position).is_err());
        let bad_output = valid_spatial_json().replace("\"widthMm\":160", "\"widthMm\":5000");
        assert!(validated_graph_json(&bad_output).is_err());
        let bad_view = valid_spatial_json().replace("\"viewHeight\":12", "\"viewHeight\":9000000");
        assert!(validated_graph_json(&bad_view).is_err());
    }

    #[test]
    fn svg_validation_rejects_script() {
        assert!(validate_export("svg", b"<svg xmlns='http://www.w3.org/2000/svg'></svg>").is_ok());
        assert!(validate_export("svg", b"<?xml version='1.0' encoding='UTF-8'?>\n<svg xmlns='http://www.w3.org/2000/svg'></svg>").is_ok());
        assert!(validate_export("svg", b"<svg><script>alert(1)</script></svg>").is_err());
        assert!(validate_export("svg", b"<?xml version='1.0'?><html></html>").is_err());
    }

    #[test]
    fn store_zip_has_local_and_end_records() {
        let zip = build_store_zip(&[("graph.json".into(), br#"{"version":1}"#.to_vec())]).unwrap();
        assert!(zip.starts_with(&0x0403_4b50u32.to_le_bytes()));
        assert!(zip.windows(4).any(|value| value == 0x0201_4b50u32.to_le_bytes()));
        assert!(zip.windows(4).any(|value| value == 0x0605_4b50u32.to_le_bytes()));
    }
}
