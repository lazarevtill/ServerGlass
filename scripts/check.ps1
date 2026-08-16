# Everything CI checks, in one command.
#
#   .\scripts\check.ps1            the Rust core
#   .\scripts\check.ps1 -All       the core, plus the Windows app and its tests
#
# The first form is the same four steps `rust:windows`, `rust:fmt`, `rust:clippy` and `rust:test`
# run, in the same order, so a green run here means a green pipeline. It exists because "the tests
# pass" says nothing about fmt and clippy, and this repository has been left red for eight commits
# over exactly that.
#
# The macOS, iOS and Android apps need their own toolchains — see docs/WINDOWS.md.
#
# --- This file is UTF-8 WITH A BOM, and that is load-bearing. ---
#
# Windows PowerShell 5.1 reads a BOM-less file as the system ANSI codepage. The em dashes in this
# file then decode as three CP1252 characters, the last of which is U+201D — which PowerShell
# treats as a closing double quote. The string terminates early and the whole script fails to parse
# with "Missing closing '}'", nowhere near the real cause. pwsh assumes UTF-8 and never sees it, so
# this breaks only on the shell a stock Windows machine actually has. Keep the BOM.

param(
    [switch]$All
)

$ErrorActionPreference = 'Stop'

function Step($name, $block) {
    Write-Host ""
    Write-Host "==> $name" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $name" -ForegroundColor Red
        exit 1
    }
}

function Need($command, $why, $install) {
    if (Get-Command $command -ErrorAction SilentlyContinue) {
        return
    }
    Write-Host "$command is not on PATH. $why" -ForegroundColor Red
    Write-Host "  $install"
    exit 1
}

Need 'cargo' 'Install Rust:' 'winget install Rustlang.Rustup; rustup default 1.89.0'

# aws-lc-sys (russh's crypto backend) compiles C and assembles x86-64 assembly during the build, so
# both of these are hard requirements rather than optional extras. Each one was a build that failed
# several minutes in with an error naming a crate rather than the missing tool.
Need 'nasm' 'aws-lc-sys assembles x86-64 assembly during the build:' 'winget install NASM.NASM'

if (-not (Test-Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC")) {
    Write-Host "The MSVC C++ build tools are missing. aws-lc-sys compiles C during the build:" -ForegroundColor Yellow
    Write-Host "  winget install Microsoft.VisualStudio.2022.BuildTools"
    Write-Host "  then add the 'Desktop development with C++' workload, which is not installed by default:"
    Write-Host '  & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify --installPath "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools" --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet --norestart'
    Write-Host "Carrying on: another Visual Studio edition may still provide the linker." -ForegroundColor DarkGray
}

rustc --version
cargo --version

Step "format" { cargo fmt --all -- --check }
Step "clippy"  { cargo clippy --workspace --all-targets --locked -- -D warnings }
Step "build"   { cargo build --workspace --locked }
Step "test"    { cargo test --workspace --locked }

if ($All) {
    Need 'dotnet' 'The Windows app needs the .NET SDK:' 'winget install Microsoft.DotNet.SDK.9'

    # The app links the core, so the core is built first and the app is pointed at it explicitly.
    #
    # Debug throughout, and the DLL path is passed rather than left to the project default. This is
    # a check — "does it compile, do the tests pass" — not a shipping build, and a debug core copied
    # into a Release output tree is the kind of quiet incoherence that is fine until it is not.
    # `scripts\build-windows.ps1 -Release` is the path that produces something to ship, and it keeps
    # the profiles matched.
    $core = Join-Path $PSScriptRoot '..\target\debug\sg_ffi.dll'
    $app = Join-Path $PSScriptRoot '..\apps\windows'

    Step "app: core"     { cargo build -p sg-ffi --locked }
    # Nothing here regenerates the bindings for real: they are committed, and this proves the
    # committed copy still matches the Rust it came from.
    Step "app: bindings" { & "$PSScriptRoot\check-bindings.ps1" }
    Step "app: tests"    { dotnet test  "$app\ServerGlass.Core.Tests\ServerGlass.Core.Tests.csproj" -c Debug --nologo "-p:SgFfiDll=$core" }
    Step "app: build"    { dotnet build "$app\ServerGlass\ServerGlass.csproj" -c Debug --nologo "-p:SgFfiDll=$core" }
}

Write-Host ""
Write-Host "All checks passed." -ForegroundColor Green
if (-not $All) {
    Write-Host "The Windows app was not built. Add -All to include it." -ForegroundColor DarkGray
}

# Deliberately not "the live tests skipped themselves": this script cannot tell whether they did,
# and asserting it would be the same trap SG_REQUIRE_FIXTURES exists to close. Say what to do to
# find out instead.
if (-not $env:SG_REQUIRE_FIXTURES) {
    Write-Host "SG_REQUIRE_FIXTURES was not set, so any live SSH test that could not reach a" -ForegroundColor DarkGray
    Write-Host "fixture skipped itself and still reported ok. To require them:" -ForegroundColor DarkGray
    Write-Host "  wsl ./fixtures/up.sh; `$env:SG_REQUIRE_FIXTURES='1'; .\scripts\check.ps1" -ForegroundColor DarkGray
    Write-Host "If the fixtures will not bind, see docs/WINDOWS.md on reserved port ranges." -ForegroundColor DarkGray
}
