use std::{fs, path::PathBuf};

fn project_file(relative: &str) -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join(relative))
        .unwrap_or_else(|error| panic!("{relative} を読み込めません: {error}"))
}

#[test]
fn nsis_installer_shows_prerequisites_without_installing_them() {
    let config: serde_json::Value =
        serde_json::from_str(&project_file("tauri.conf.json")).expect("tauri.conf.jsonが不正です");
    assert_eq!(
        config["bundle"]["windows"]["nsis"]["installerHooks"],
        "windows/installer-hooks.nsh"
    );
    assert_eq!(
        config["bundle"]["windows"]["nsis"]["languages"][0],
        "Japanese"
    );

    let hook = project_file("windows/installer-hooks.nsh");
    for required in [
        "NSIS_HOOK_POSTINSTALL",
        "TeX Live",
        "uplatex",
        "dvipdfmx",
        "OpenAI Codex CLI",
        "ChatGPT",
        "Node.js",
        "Tailscale",
        "任意",
        "自動インストールされません",
    ] {
        assert!(hook.contains(required), "インストーラー案内に {required} がありません");
    }
    assert!(
        !hook.contains("ExecWait") && !hook.contains("nsExec") && !hook.contains("install.ps1"),
        "インストーラーから外部環境を自動実行しないでください"
    );
}

#[test]
fn first_run_guide_uses_live_detection_and_can_be_reopened() {
    let app = project_file("../src/App.tsx");
    assert!(app.contains("setup_guide_completed"));
    assert!(app.contains("<SetupGuide"));
    assert!(app.contains("setView(\"settings\")"));

    let guide = project_file("../src/components/SetupGuide.tsx");
    assert!(guide.contains("tailscaleStatus()"));
    assert!(guide.contains("tex?.uplatex_path"));
    assert!(guide.contains("tex?.dvipdfmx_path"));
    assert!(guide.contains("codex?.installed"));
    assert!(guide.contains("codex?.account?.account"));
    assert!(guide.contains("tailscale?.installed"));
    assert!(guide.contains("tailscale?.serveConfigured"));
    assert!(guide.contains("同じWi-Fi内だけで使う場合は不要です"));
    assert!(guide.contains("教材工房が外部ソフトを無断でインストールすることはありません"));

    let settings = project_file("../src/components/SettingsView.tsx");
    assert!(settings.contains("セットアップ診断"));
    assert!(settings.contains("環境と導入手順を確認"));
}

#[test]
fn codex_guide_distinguishes_windows_installer_from_npm_requirements() {
    let requirements = project_file("../src/setupRequirements.ts");
    assert!(requirements.contains("https://chatgpt.com/codex/install.ps1"));
    assert!(requirements.contains("npm install -g @openai/codex"));
    assert!(requirements.contains("https://nodejs.org/en/download"));
    assert!(requirements.contains("https://tailscale.com/docs/install/windows"));
    assert!(requirements.contains("https://tailscale.com/docs/install/ios"));

    let guide = project_file("../src/components/SetupGuide.tsx");
    assert!(guide.contains("npm版を使う場合だけNode.jsが必要です"));
    assert!(guide.contains("CODEX_WINDOWS_INSTALL_COMMAND"));
    assert!(guide.contains("CODEX_NPM_INSTALL_COMMAND"));
}
