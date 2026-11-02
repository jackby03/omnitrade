# Dauerlauf: omnitrade live auf OKX-Demo (EEA), BTC-USDC, Kapital 300.
# Die Demo-Schluessel kommen aus der Umgebungsvariable OKX_DEMO_API_KEY/_SECRET/_PASSPHRASE
# des Benutzers und werden nur in den Prozess dieses Skripts uebernommen. Sie stehen in keiner Datei.
# Stoppen: den Wrapper beenden (PID in logs\live_okx_demo.wrapper.pid) und danach den CLI-Prozess.
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\live-okx-demo.ps1
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Cli = Join-Path $Root 'target-fix\debug\omnitrade-cli.exe'
$Logs = Join-Path $Root 'logs'
$Status = 'status/live_300.json'
$Out = Join-Path $Logs 'live_okx_demo.out.log'
$Err = Join-Path $Logs 'live_okx_demo.err.log'
$WrapperPid = Join-Path $Logs 'live_okx_demo.wrapper.pid'

New-Item -ItemType Directory -Force -Path $Logs | Out-Null
Set-Content -Path $WrapperPid -Value $PID

if (-not $env:OKX_DEMO_API_KEY -or -not $env:OKX_DEMO_API_SECRET -or -not $env:OKX_DEMO_API_PASSPHRASE) {
    throw 'OKX_DEMO_API_KEY, OKX_DEMO_API_SECRET oder OKX_DEMO_API_PASSPHRASE fehlt in der Umgebung.'
}
$env:OKX_API_KEY = $env:OKX_DEMO_API_KEY
$env:OKX_API_SECRET = $env:OKX_DEMO_API_SECRET
$env:OKX_PASSPHRASE = $env:OKX_DEMO_API_PASSPHRASE
$env:OKX_REGION = 'eea'

$liveArgs = @('live', '--script', 'strategien/ema_cross_20_50.omni', '--symbol', 'BTC-USDC',
          '--exchange', 'okx', '--network', 'testnet',
          '--initial-capital', '300', '--max-notional-usd', '300', '--status-file', $Status)

while ($true) {
    Add-Content -Path $Out -Value "$(Get-Date -Format s) start"
    $p = Start-Process -FilePath $Cli -ArgumentList $liveArgs -WorkingDirectory $Root -PassThru -Wait `
        -RedirectStandardOutput $Out -RedirectStandardError $Err -NoNewWindow
    Add-Content -Path $Out -Value "$(Get-Date -Format s) beendet mit Code $($p.ExitCode), Neustart in 30 s"
    Start-Sleep -Seconds 30
}
