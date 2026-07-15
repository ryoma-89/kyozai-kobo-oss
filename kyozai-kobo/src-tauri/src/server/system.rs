//! Windows運用の補助: Tailscale状態確認・ログイン時自動起動（レジストリRun）

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn find_tailscale() -> Option<PathBuf> {
    if let Ok(out) = no_window(Command::new("where.exe").arg("tailscale")).output() {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let p = PathBuf::from(line.trim());
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let default = PathBuf::from("C:\\Program Files\\Tailscale\\tailscale.exe");
    if default.exists() {
        return Some(default);
    }
    None
}

fn run_capture(cmd: &mut Command) -> Result<String, String> {
    let out = no_window(cmd)
        .output()
        .map_err(|e| format!("コマンドを実行できません: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        let code = out
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "不明".to_string());
        return Err(if detail.is_empty() {
            format!("コマンドが終了コード {} で失敗しました", code)
        } else {
            format!("コマンドが終了コード {} で失敗しました: {}", code, detail)
        });
    }
    if stdout.trim().is_empty() {
        Ok(stderr)
    } else {
        Ok(stdout)
    }
}

fn contains_serve_target(value: &Value, port: u16) -> bool {
    match value {
        Value::String(text) => contains_serve_target_text(text, port),
        Value::Array(values) => values.iter().any(|value| contains_serve_target(value, port)),
        Value::Object(values) => values
            .values()
            .any(|value| contains_serve_target(value, port)),
        _ => false,
    }
}

fn contains_serve_target_text(text: &str, port: u16) -> bool {
    text.contains(&format!("127.0.0.1:{}", port))
        || text.contains(&format!("localhost:{}", port))
}

fn parse_serve_status_json(output: &str, port: u16) -> Result<(String, bool), String> {
    let parsed: Value = serde_json::from_str(output)
        .map_err(|e| format!("serve status --json の応答を解析できません: {}", e))?;
    let configured = contains_serve_target(&parsed, port);
    let display = serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| output.to_string());
    Ok((display, configured))
}

fn read_serve_status(ts: &std::path::Path, port: u16) -> Result<(String, bool), String> {
    let json_error = match run_capture(Command::new(ts).args(["serve", "status", "--json"])) {
        Ok(output) => match parse_serve_status_json(&output, port) {
            Ok(status) => return Ok(status),
            Err(error) => error,
        },
        Err(error) => error,
    };

    // --json 未対応の旧CLIだけ、人間向け出力へ後方互換フォールバックする。
    match run_capture(Command::new(ts).args(["serve", "status"])) {
        Ok(output) => {
            let configured = contains_serve_target_text(&output, port);
            Ok((output, configured))
        }
        Err(error) => Err(format!(
            "Serve状態を取得できません（JSON: {}; 通常出力: {}）",
            json_error, error
        )),
    }
}

/// Tailscaleのインストール・接続・Serve設定状態を返す
pub fn tailscale_status(state: &AppState) -> Result<Value, String> {
    let port = super::configured_port(state);
    let Some(ts) = find_tailscale() else {
        return Ok(json!({
            "installed": false,
            "message": "Tailscaleが見つかりません。https://tailscale.com/download からインストールしてください。",
        }));
    };

    let (version, version_error) = match run_capture(Command::new(&ts).arg("version")) {
        Ok(output) => (
            output
                .lines()
                .next()
                .map(|line| line.trim().to_string())
                .unwrap_or_default(),
            None,
        ),
        Err(error) => (String::new(), Some(error)),
    };

    let (parsed, status_error) = match run_capture(Command::new(&ts).args(["status", "--json"])) {
        Ok(output) => match serde_json::from_str::<Value>(&output) {
            Ok(parsed) => (parsed, None),
            Err(error) => (
                Value::Null,
                Some(format!("status --json の応答を解析できません: {}", error)),
            ),
        },
        Err(error) => (Value::Null, Some(error)),
    };
    let backend_state = parsed
        .get("BackendState")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let dns_name = parsed
        .pointer("/Self/DNSName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('.')
        .to_string();

    let (serve_status, serve_configured, serve_status_error) =
        match read_serve_status(&ts, port) {
            Ok((status, configured)) => (status, configured, None),
            Err(error) => (String::new(), false, Some(error)),
        };

    let https_url = if dns_name.is_empty() {
        String::new()
    } else {
        format!("https://{}", dns_name)
    };

    Ok(json!({
        "installed": true,
        "version": version,
        "versionError": version_error,
        "backendState": backend_state,
        "connected": backend_state == "Running",
        "statusError": status_error,
        "dnsName": dns_name,
        "httpsUrl": https_url,
        "serveConfigured": serve_configured,
        "serveStatus": serve_status.trim(),
        "serveStatusError": serve_status_error,
        "suggestedCommand": format!("tailscale serve --bg --https=443 http://127.0.0.1:{}", port),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_json_finds_configured_local_target() {
        let status = json!({
            "TCP": {"443": {"HTTPS": true}},
            "Web": {
                "device.example.ts.net:443": {
                    "Handlers": {"/": {"Proxy": "http://127.0.0.1:8760"}}
                }
            }
        })
        .to_string();

        let (_, configured) = parse_serve_status_json(&status, 8760).unwrap();
        assert!(configured);
    }

    #[test]
    fn serve_json_rejects_a_different_target_port() {
        let status = json!({
            "Web": {"host:443": {"Handlers": {"/": {"Proxy": "http://localhost:9000"}}}}
        })
        .to_string();

        let (_, configured) = parse_serve_status_json(&status, 8760).unwrap();
        assert!(!configured);
    }
}

// ---- ログイン時自動起動（HKCU Run） ----

const RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "KyozaiKobo";

pub fn autostart_get() -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        let out = no_window(Command::new("reg").args(["query", RUN_KEY, "/v", RUN_VALUE]))
            .output()
            .map_err(|e| e.to_string())?;
        Ok(out.status.success())
    }
}

pub fn autostart_set(enabled: bool) -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        let _ = enabled;
        return Err("Windows以外では未対応です".into());
    }
    #[cfg(windows)]
    {
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let value = format!("\"{}\"", exe.to_string_lossy());
            let out = no_window(Command::new("reg").args([
                "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &value, "/f",
            ]))
            .output()
            .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!(
                    "レジストリへの登録に失敗しました: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            Ok(true)
        } else {
            let out = no_window(Command::new("reg").args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]))
                .output()
                .map_err(|e| e.to_string())?;
            // 値が無い場合の削除失敗は成功扱い
            let _ = out;
            Ok(false)
        }
    }
}
