# Fragt Binance-Testnet-Key und -Secret verdeckt ab und legt sie im
# Windows Credential Manager ab (Eintrag omnitrade/binance/testnet).
# Die Werte stehen nur für die Dauer dieses Aufrufs in der Umgebung.
$ErrorActionPreference = 'Stop'

function Read-Plain([string]$prompt) {
    $s = Read-Host -Prompt $prompt -AsSecureString
    $b = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s)
    try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($b) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($b) }
}

try {
    $env:OMNITRADE_API_KEY = (Read-Plain 'Testnet API-Key').Trim()
    $env:OMNITRADE_API_SECRET = (Read-Plain 'Testnet Secret').Trim()
    Push-Location (Join-Path $PSScriptRoot '..')
    cargo run -q -p omnitrade-exchange --example keystore_set
} finally {
    Pop-Location -ErrorAction SilentlyContinue
    Remove-Item Env:OMNITRADE_API_KEY, Env:OMNITRADE_API_SECRET -ErrorAction SilentlyContinue
}
