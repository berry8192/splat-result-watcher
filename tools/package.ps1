# 配布用の zip を作る（dist\splat-result-watcher-v<版>.zip）。
# 画面を組み、release で組み、ライブラリのライセンス一覧（THIRD_PARTY_LICENSES.html）を作って、まとめて zip にする。
# 要るもの: Node.js、Rust、cargo-about（cargo install cargo-about --locked --features cli）
# 使い方: powershell -ExecutionPolicy Bypass -File tools\package.ps1 [-TargetDir target\pkg]
#   GUI を起動したままでも組めるよう、既定では target\release とは別の場所に組む
param(
    [string]$TargetDir = "target\pkg"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Run($exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') が失敗しました（$LASTEXITCODE）" }
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "splat-result-watcher-v$version"
Write-Host "== $name"

# 1. 画面（exe に埋め込む）
Run npm @("--prefix", "ui", "ci")
Run npm @("--prefix", "ui", "run", "build")

# 2. exe
Run cargo @("build", "--release", "--locked", "--target-dir", $TargetDir)
$bin = Join-Path $TargetDir "release"

# 3. ライセンス一覧。Rust は cargo-about、画面のライブラリ（exe に入る dependencies だけ）は node_modules から
$out = Join-Path $root "dist\$name"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null
$html = Join-Path $out "THIRD_PARTY_LICENSES.html"
Run cargo @("about", "generate", "--locked", "-o", $html, "about.hbs")

$npm = New-Object System.Text.StringBuilder
[void]$npm.AppendLine("<h2>画面（JavaScript）のライブラリ</h2>")
$paths = & npm --prefix ui ls --omit=dev --all --parseable
foreach ($dir in ($paths | Select-Object -Skip 1 | Sort-Object -Unique)) {
    $pkg = Get-Content (Join-Path $dir "package.json") -Raw -Encoding UTF8 | ConvertFrom-Json
    $lic = Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING)' } | Select-Object -First 1
    $text = if ($lic) { Get-Content $lic.FullName -Raw -Encoding UTF8 } else { "（ライセンスのファイルが含まれていません。ライセンス: $($pkg.license)）" }
    [void]$npm.AppendLine("<h3>$($pkg.name) $($pkg.version)（$($pkg.license)）</h3>")
    [void]$npm.AppendLine("<pre>$([System.Net.WebUtility]::HtmlEncode($text))</pre>")
}
[void]$npm.AppendLine("<h2>フォント</h2>")
[void]$npm.AppendLine("<h3>M PLUS 1p（SIL Open Font License 1.1）</h3>")
[void]$npm.AppendLine("<pre>$([System.Net.WebUtility]::HtmlEncode((Get-Content ui\public\fonts\OFL.txt -Raw -Encoding UTF8)))</pre>")
$body = [System.IO.File]::ReadAllText($html, [System.Text.Encoding]::UTF8)
$body = $body.Replace("<!-- NPM_LICENSES -->", $npm.ToString())
[System.IO.File]::WriteAllText($html, $body, (New-Object System.Text.UTF8Encoding $false))

# 4. まとめる
Copy-Item (Join-Path $bin "splat-result-watcher-gui.exe") $out
Copy-Item (Join-Path $bin "splat-result-watcher.exe") $out
Copy-Item LICENSE (Join-Path $out "LICENSE.txt")
Copy-Item README.md $out
Copy-Item ui\public\fonts\OFL.txt (Join-Path $out "OFL.txt")

$zip = Join-Path $root "dist\$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $out -DestinationPath $zip
Write-Host "== できました: $zip"
