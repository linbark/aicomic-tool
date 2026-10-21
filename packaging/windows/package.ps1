param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipBuild,
    [string]$Binary,
    [string]$Output
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$RepoDir = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
if ($Target -ne "x86_64-pc-windows-msvc") { throw "Only Windows x64 MSVC is packaged." }
$Manifest = Get-Content (Join-Path $RepoDir "Cargo.toml") -Raw
$Version = [regex]::Match($Manifest, '(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"').Groups[1].Value
if (-not $Version) { throw "Workspace release version was not found." }

function Assert-StandaloneBinary([string]$Executable) {
    $Tool = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($Tool) { $Dumpbin = $Tool.Source }
    else {
        $VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
        if (-not (Test-Path $VsWhere)) { throw "MSVC dumpbin.exe is required to verify release dependencies." }
        $VsRoot = & $VsWhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $Tool = Get-ChildItem (Join-Path $VsRoot "VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe") | Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $Tool) { throw "MSVC dumpbin.exe was not found." }
        $Dumpbin = $Tool.FullName
    }
    $Dependencies = & $Dumpbin /DEPENDENTS $Executable 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Release dependency inspection failed." }
    $Text = $Dependencies -join "`n"
    if ($Text -match '(?i)\b(vcruntime[0-9_]*|msvcp[0-9_]*|concrt[0-9_]*|python[0-9]*|node|webview2loader)\.dll\b') {
        throw "The executable depends on an unbundled runtime DLL. Build Windows with -C target-feature=+crt-static. Imports:`n$Text"
    }
    Write-Output $Text
}

Push-Location $RepoDir
try {
    if (-not $SkipBuild -and -not $Binary) {
        $PreviousTargetFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
        try {
            $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = "$PreviousTargetFlags -C target-feature=+crt-static".Trim()
            & cargo build --locked -p aicomic-desktop --release --target $Target
            if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed." }
        }
        finally { $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $PreviousTargetFlags }
    }
    if (-not $Binary) { $Binary = Join-Path $RepoDir "target/$Target/release/aicomic-tool.exe" }
    $Binary = (Resolve-Path $Binary).Path
    Assert-StandaloneBinary $Binary
    if (-not $Output) { $Output = Join-Path $RepoDir "dist/windows-x64" }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $Output = (Resolve-Path $Output).Path
    $Portable = Join-Path $Output "Xingqing-$Version-windows-x64.exe"
    Copy-Item $Binary $Portable -Force
    $Installer = Join-Path $Output "Xingqing-$Version-windows-x64-setup.exe"
    $Compiler = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($Compiler) { $CompilerPath = $Compiler.Source }
    else { $CompilerPath = Join-Path ${env:ProgramFiles(x86)} "NSIS/makensis.exe" }
    if (-not (Test-Path $CompilerPath)) { throw "NSIS is required for the Windows installer. Install NSIS 3 and put makensis.exe on PATH." }
    & $CompilerPath /V3 /WX /INPUTCHARSET UTF8 "/DAPP_VERSION=$Version" "/DBINARY=$Binary" "/DOUTPUT=$Installer" (Join-Path $PSScriptRoot "installer.nsi")
    if ($LASTEXITCODE -ne 0) { throw "NSIS packaging failed." }
    if (-not (Test-Path $Installer)) { throw "NSIS did not produce the installer." }
    Get-FileHash $Portable, $Installer -Algorithm SHA256 | Format-Table
    Write-Output "Created $Output. Executable and installer are unsigned."
}
finally { Pop-Location }
