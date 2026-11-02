# Treiber fuer omnitrade unter Windows: GUI (Start, Screenshot, Klicks) und CLI-Smoke.
# Aufruf aus dem Repo-Root:
#   powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\run-omnitrade\driver.ps1 <befehl>
# Befehle: build | flow | start | shot [name] | killswitch | reset | status | stop | cli
param(
    [Parameter(Position = 0)] [string]$Cmd = 'help',
    [Parameter(Position = 1)] [string]$Name = 'screen'
)

$ErrorActionPreference = 'Stop'
$SkillDir = $PSScriptRoot
$Root = (Resolve-Path (Join-Path $SkillDir '..\..\..')).Path
$Run = Join-Path $Root 'target\run-omnitrade'
$Bin = Join-Path $Root 'target\debug'
$KillFile = Join-Path $Run 'omnitrade.kill'
$Symbol = if ($env:GUI_SYMBOL) { $env:GUI_SYMBOL } else { 'BTC-USDT' }
$Exchange = if ($env:GUI_EXCHANGE) { $env:GUI_EXCHANGE } else { 'okx' }
# Klickpunkte relativ zur Fensterecke oben links (inkl. Titelleiste), physische Pixel.
# Layout-abhaengig: nach jeder UI-Aenderung per Screenshot pruefen.
$KillXY = @(398, 42)
$ResetXY = @(522, 42)

New-Item -ItemType Directory -Force -Path $Run | Out-Null
$env:OMNITRADE_KILL_FILE = $KillFile

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Win32 {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
Add-Type -AssemblyName System.Drawing
[void][Win32]::SetProcessDPIAware()

function Get-Gui {
    Get-Process -Name 'omnitrade-ui' -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
}

function Wait-Gui {
    for ($i = 0; $i -lt 40; $i++) {
        if (Get-Gui) { return }
        Start-Sleep -Seconds 1
    }
    throw "kein GUI-Fenster nach 40 s - siehe $Run\ui.log"
}

function Get-GuiRect {
    $p = Get-Gui
    if (-not $p) { throw 'kein GUI-Fenster - zuerst: driver.ps1 start' }
    $r = New-Object Win32+RECT
    [void][Win32]::GetWindowRect($p.MainWindowHandle, [ref]$r)
    return $r
}

# Klickt, bis $Done wahr ist, hoechstens dreimal. $Done wird nach JEDEM Klick geprueft,
# damit ein Wiederholen nie zurueckschaltet (Doppelklick hat auf diesem Host den Not-Aus
# mehrfach hin- und hergeschaltet). Der erste Klick nach einer Mausbewegung geht bei egui
# oefter verloren, daher die Wiederholung.
function Invoke-GuiClick([int]$x, [int]$y, [scriptblock]$Done) {
    $r = Get-GuiRect
    [void][Win32]::SetForegroundWindow((Get-Gui).MainWindowHandle)
    Start-Sleep -Milliseconds 300
    [void][Win32]::SetCursorPos($r.Left + $x, $r.Top + $y)
    Start-Sleep -Milliseconds 600
    for ($k = 0; $k -lt 3; $k++) {
        if (& $Done) { break }
        [Win32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
        [Win32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
    }
}

function Invoke-Start {
    if (Get-Gui) { Write-Output 'GUI laeuft bereits'; return }
    $bin = Join-Path $Bin 'omnitrade-ui.exe'
    if (-not (Test-Path $bin)) { throw 'omnitrade-ui.exe fehlt - zuerst: driver.ps1 build' }
    Remove-Item -Force -ErrorAction SilentlyContinue $KillFile
    $proc = Start-Process -FilePath $bin -ArgumentList @($Symbol, 'testnet', '--exchange', $Exchange) `
        -WorkingDirectory $Root -PassThru -WindowStyle Normal `
        -RedirectStandardOutput (Join-Path $Run 'ui.log') -RedirectStandardError (Join-Path $Run 'ui.err.log')
    Set-Content -Path (Join-Path $Run 'ui.pid') -Value $proc.Id
    Wait-Gui
    Start-Sleep -Seconds 8   # Kerzen-Laden abwarten
    Write-Output "GUI gestartet (PID $($proc.Id))"
}

function Invoke-Shot([string]$shotName) {
    $r = Get-GuiRect
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $h))
    $out = Join-Path $Run "$shotName.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Output $out
}

function Invoke-Killswitch {
    Invoke-GuiClick $KillXY[0] $KillXY[1] { Test-Path $KillFile }
    if (-not (Test-Path $KillFile)) { throw 'Flag-Datei fehlt nach Klick auf NOT-AUS' }
    Write-Output "OK: NOT-AUS gesetzt, Flag-Datei $KillFile"
    Invoke-Shot 'after_killswitch'
}

function Invoke-Reset {
    Invoke-GuiClick $ResetXY[0] $ResetXY[1] { -not (Test-Path $KillFile) }
    if (Test-Path $KillFile) {
        # Der Klick auf "Zurücksetzen" kam auf diesem Host nicht an (Ursache offen, siehe SKILL.md).
        # Fallback: dieselbe Flag-Datei ueber die CLI aufheben.
        Write-Output 'WARNUNG: GUI-Klick auf den Reset-Knopf ohne Wirkung - Fallback: omnitrade-cli kill --reset'
        $cli = Join-Path $Bin 'omnitrade-cli.exe'
        & $cli kill --reset
        if (Test-Path $KillFile) { throw 'Flag-Datei existiert auch nach CLI-Reset noch' }
    }
    Write-Output 'OK: NOT-AUS zurueckgesetzt'
    Invoke-Shot 'after_reset'
}

function Invoke-Status {
    if (Get-Gui) { Write-Output 'GUI: laeuft' } else { Write-Output 'GUI: aus' }
    if (Test-Path $KillFile) { Write-Output 'NOT-AUS: gesetzt' } else { Write-Output 'NOT-AUS: frei' }
}

function Invoke-Stop {
    $pidFile = Join-Path $Run 'ui.pid'
    if (Test-Path $pidFile) {
        $id = [int](Get-Content $pidFile)
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
        Remove-Item -Force $pidFile
    }
    Write-Output 'gestoppt'
}

function Invoke-Fetch {
    $csv = Join-Path $Run 'btc_1h.csv'
    $url = 'https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1H&limit=100'
    $d = Invoke-RestMethod -Uri $url -TimeoutSec 20
    if ($d.code -ne '0' -or -not $d.data) { throw "OKX-Antwort ungueltig: $($d.msg)" }
    $rows = $d.data | Sort-Object { [int64]$_[0] }
    $lines = @('timestamp_ms,open,high,low,close,volume')
    foreach ($r in $rows) { $lines += ($r[0..5] -join ',') }
    [IO.File]::WriteAllLines($csv, $lines, (New-Object Text.UTF8Encoding $false))
    Write-Output "rows $($rows.Count) -> $csv"
}

function Invoke-Cli {
    $cli = Join-Path $Bin 'omnitrade-cli.exe'
    if (-not (Test-Path $cli)) { throw 'omnitrade-cli.exe fehlt - zuerst: driver.ps1 build' }
    Invoke-Fetch
    $csv = Join-Path $Run 'btc_1h.csv'
    & $cli validate --script (Join-Path $SkillDir 'sample_buy.omni')
    & $cli validate --script (Join-Path $SkillDir 'sample_rsi.omni')
    & $cli backtest --script (Join-Path $SkillDir 'sample_buy.omni') --data $csv --initial-balance 1000000
    & $cli backtest --script (Join-Path $SkillDir 'sample_rsi.omni') --data $csv --initial-balance 1000000
}

switch ($Cmd) {
    'build' { Set-Location $Root; cargo build --workspace }
    'flow' { Invoke-Start; Invoke-Shot '01_start'; Invoke-Killswitch; Invoke-Reset; Invoke-Status }
    'start' { Invoke-Start }
    'shot' { Invoke-Shot $Name }
    'killswitch' { Invoke-Killswitch }
    'reset' { Invoke-Reset }
    'status' { Invoke-Status }
    'stop' { Invoke-Stop }
    'fetch' { Invoke-Fetch }
    'cli' { Invoke-Cli }
    default { Write-Output 'Befehle: build | flow | start | shot [name] | killswitch | reset | status | stop | fetch | cli'; exit 2 }
}
