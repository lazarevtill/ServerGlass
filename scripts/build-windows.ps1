# Build the Windows app, core and all.
#
#   .\scripts\build-windows.ps1                 debug build, into apps/windows/ServerGlass/bin
#   .\scripts\build-windows.ps1 -Release        release build
#   .\scripts\build-windows.ps1 -Release -Publish   a self-contained folder you can copy anywhere
#
# The Windows twin of scripts/build-macos.sh: compile the Rust cdylib, regenerate the C# bindings
# from it, then build the app against both. The order matters — the app's project file fails the
# build outright if sg_ffi.dll is absent, because an app that compiles without the core only fails
# when somebody runs it.
#
# UTF-8 with a BOM, for the reason spelled out at the top of check.ps1.

param(
    [switch]$Release,
    [switch]$Publish
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$cargoProfile = if ($Release) { 'release' } else { 'debug' }
$configuration = if ($Release) { 'Release' } else { 'Debug' }
$dll = Join-Path $root "target\$cargoProfile\sg_ffi.dll"

function Step($name, $block) {
    Write-Host ""
    Write-Host "==> $name" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $name" -ForegroundColor Red
        exit 1
    }
}

Step "rust core ($cargoProfile)" {
    if ($Release) { cargo build --release -p sg-ffi } else { cargo build -p sg-ffi }
}

if (-not (Test-Path $dll)) {
    Write-Host "cargo reported success but $dll is missing." -ForegroundColor Red
    exit 1
}

Step "c# bindings" { cargo run --quiet -p sg-bindgen --bin csharp-bindgen }

Step "app tests" {
    dotnet test (Join-Path $root 'apps\windows\ServerGlass.Core.Tests\ServerGlass.Core.Tests.csproj') `
        -c $configuration --nologo "-p:SgFfiDll=$dll"
}

if ($Publish) {
    Step "publish" {
        dotnet publish (Join-Path $root 'apps\windows\ServerGlass\ServerGlass.csproj') `
            -c $configuration --nologo "-p:SgFfiDll=$dll"
    }
} else {
    Step "app" {
        dotnet build (Join-Path $root 'apps\windows\ServerGlass\ServerGlass.csproj') `
            -c $configuration --nologo "-p:SgFfiDll=$dll"
    }
}

$output = Join-Path $root "apps\windows\ServerGlass\bin\$configuration\net9.0-windows10.0.19041.0\win-x64"
Write-Host ""
Write-Host "Built." -ForegroundColor Green
Write-Host "  $output\ServerGlass.exe"
