# 教材工房: Tailscale Serve セットアップスクリプト
# 教材サーバー(127.0.0.1のみで待受)を、tailnet内の自分の端末だけへHTTPSで公開します。
# 使い方:  PowerShellで  .\setup-tailscale-serve.ps1 [-Port 8760]
param(
    [int]$Port = 8760
)

$ErrorActionPreference = "Stop"

function Find-Tailscale {
    $cmd = Get-Command tailscale -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $default = "C:\Program Files\Tailscale\tailscale.exe"
    if (Test-Path $default) { return $default }
    return $null
}

$ts = Find-Tailscale
if (-not $ts) {
    Write-Host "Tailscaleが見つかりません。https://tailscale.com/download からインストールしてください。" -ForegroundColor Red
    exit 1
}

Write-Host "Tailscale: $ts"
& $ts version | Select-Object -First 1

# 接続状態の確認
$statusJson = & $ts status --json | ConvertFrom-Json
if ($statusJson.BackendState -ne "Running") {
    Write-Host "Tailscaleが未接続です（状態: $($statusJson.BackendState)）。" -ForegroundColor Yellow
    Write-Host "タスクトレイのTailscaleからログインしてから再実行してください。"
    exit 1
}

$dns = $statusJson.Self.DNSName.TrimEnd(".")
Write-Host "この端末のtailnet名: $dns"

# 教材サーバーの稼働確認
try {
    $health = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/api/health" -TimeoutSec 3
    Write-Host "教材サーバー稼働確認: OK (v$($health.version))" -ForegroundColor Green
} catch {
    Write-Host "警告: http://127.0.0.1:$Port に教材サーバーが見つかりません。" -ForegroundColor Yellow
    Write-Host "教材工房アプリの「設定 → 教材サーバー」からサーバーを起動してください（Serve設定自体はこのまま行えます）。"
}

# Serve設定（tailnet内のみ。Funnelによる一般公開は行わない）
Write-Host "`ntailscale serve を設定します: https://$dns -> http://127.0.0.1:$Port"
& $ts serve --bg --https=443 "http://127.0.0.1:$Port"
if ($LASTEXITCODE -ne 0) {
    Write-Host "serveコマンドに失敗しました。Tailscaleのバージョンによっては次を試してください:" -ForegroundColor Yellow
    Write-Host "  tailscale serve --bg localhost:$Port"
    exit 1
}

Write-Host "`n===== 設定完了 =====" -ForegroundColor Green
& $ts serve status
Write-Host ""
Write-Host "iPad等（同じtailnetにログインした端末）から次のURLで開けます:" -ForegroundColor Cyan
Write-Host "  https://$dns" -ForegroundColor Cyan
Write-Host ""
Write-Host "全Serve設定を解除するには:  tailscale serve reset"
Write-Host "Tailscale Servicesの個別設定を解除するには:  tailscale serve clear svc:<service-name>"
Write-Host "注意: 'tailscale funnel' は使用しないでください（インターネットへ一般公開されます）。"
