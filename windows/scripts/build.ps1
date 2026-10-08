# Собирает Folio для Windows: веб-часть, утилиту folio, приложение и установщик NSIS.
# Использование: windows\scripts\build.ps1 [-SkipTests]
# Результат: windows\target\release\bundle\nsis\Folio_<версия>_x64-setup.exe
param([switch]$SkipTests)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$windows = Join-Path $root 'windows'
$version = (Get-Content (Join-Path $root 'VERSION') -Raw).Trim()

# Инструменты, поставленные в C:\src без записи в PATH.
foreach ($folder in 'C:\src\cargo\bin', 'C:\src\node') {
    if ((Test-Path $folder) -and -not (($env:Path -split ';') -contains $folder)) { $env:Path = "$folder;$env:Path" }
}
if (-not $env:RUSTUP_HOME -and (Test-Path 'C:\src\rustup')) { $env:RUSTUP_HOME = 'C:\src\rustup'; $env:CARGO_HOME = 'C:\src\cargo' }

function Invoke-Step([string]$title, [scriptblock]$command) {
    Write-Host "==> $title"
    # cargo и npm пишут ход работы в stderr; Windows PowerShell 5.1 при перенаправлении вывода счёл бы это ошибкой.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $command 2>&1 | ForEach-Object { "$_" } } finally { $ErrorActionPreference = $previous }
    if ($LASTEXITCODE -ne 0) { throw "$title — ошибка (код $LASTEXITCODE)" }
}

$cargoVersion = (Select-String -Path (Join-Path $windows 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if ($cargoVersion -ne $version) { throw "Версия в windows\Cargo.toml ($cargoVersion) не совпадает с файлом VERSION ($version)" }

Write-Host "==> Folio $version для Windows"

Push-Location (Join-Path $root 'web')
try {
    if (-not (Test-Path 'node_modules')) { Invoke-Step 'веб-часть: зависимости' { npm ci --no-audit --no-fund } }
    if (-not $SkipTests) { Invoke-Step 'веб-часть: тесты' { npm test } }
    Invoke-Step 'веб-часть: сборка' { npm run build }
} finally { Pop-Location }

Push-Location $windows
try {
    if (-not (Test-Path 'node_modules')) { Invoke-Step 'интерфейс: зависимости' { npm ci --no-audit --no-fund } }
    if (-not $SkipTests) {
        Invoke-Step 'интерфейс: проверка типов' { npm run typecheck }
        Invoke-Step 'ядро и утилита: тесты' { cargo test --workspace }
    }
    Invoke-Step 'утилита folio' { cargo build --release -p folio-cli }

    # Утилита кладётся в bin рядом с Folio.exe: имена folio.exe и Folio.exe в одной папке Windows не различает.
    $overlay = @{ version = $version; bundle = @{ resources = @{ '../target/release/folio-cli.exe' = 'bin/folio.exe' } } }
    $key = Join-Path $env:USERPROFILE '.tauri\folio-updater.key'
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and (Test-Path $key)) {
        $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $key -Raw).Trim()
        if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = '' }
    }
    if ($env:TAURI_SIGNING_PRIVATE_KEY) {
        $overlay.bundle.createUpdaterArtifacts = $true
    } else {
        Write-Host '    ключ подписи обновлений не найден: установщик соберётся без файлов автообновления'
    }
    $overlayPath = Join-Path $env:TEMP 'folio-tauri-release.json'
    [System.IO.File]::WriteAllText($overlayPath, ($overlay | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding $false))

    Invoke-Step 'приложение и установщик' { npx tauri build --config $overlayPath }
} finally { Pop-Location }

$bundle = Join-Path $windows 'target\release\bundle\nsis'
Write-Host "готово: $bundle"
Get-ChildItem $bundle | ForEach-Object { Write-Host "  $($_.Name)  $([math]::Round($_.Length / 1MB, 1)) МБ" }
