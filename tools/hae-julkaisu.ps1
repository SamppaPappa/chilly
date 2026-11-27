# Hakee GitHubin rakentaman allekirjoitetun Windows-asentimen (+ .sig) paikkaan,
# josta tools/julkaise.py sen löytää. Käyttö (Chillyn kansiossa):
#   powershell -File tools\hae-julkaisu.ps1            # versio tauri.conf.jsonista
#   powershell -File tools\hae-julkaisu.ps1 0.2.3
param([string]$Versio)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $Versio) { $Versio = (Get-Content "$root\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version }
$kohde = "$root\src-tauri\target\release\bundle\nsis"
New-Item -ItemType Directory -Force $kohde | Out-Null
gh release download "v$Versio" --repo SamppaPappa/chilly --pattern "Chilly_${Versio}_x64-setup.exe*" --dir $kohde --clobber
Get-ChildItem $kohde -Filter "Chilly_${Versio}_x64-setup.exe*" | Select-Object Name, Length
Write-Host "Valmis. Seuraavaksi: python -X utf8 tools\julkaise.py ""muutokset"" [lisäkuvat...]"
