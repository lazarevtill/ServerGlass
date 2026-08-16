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
    [switch]$Publish,
    # Stamped into the executable. scripts/package-windows.ps1 passes the workspace version, so the
    # binary, the installer and the Apps & Features entry all say the same number. Left off, MSBuild
    # uses its own default of 1.0.0.0 - which is harmless in a dev build and wrong on the version
    # line of a crash report, where it was first noticed.
    [string]$Version
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$cargoProfile = if ($Release) { 'release' } else { 'debug' }
$configuration = if ($Release) { 'Release' } else { 'Debug' }
$dll = Join-Path $root "target\$cargoProfile\sg_ffi.dll"
# An empty array contributes no arguments at all, so the unversioned form stays exactly as it was.
$versionArgument = if ($Version) { @("-p:Version=$Version") } else { @() }

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
    # SelfContained here and nowhere else. The project builds framework-dependent, which is right
    # for a dev loop and for CI - both have the SDK, and carrying the runtime through every build
    # would cost a couple of hundred megabytes for nothing. But "a folder you can copy anywhere"
    # has to mean anywhere, and a framework-dependent folder needs the .NET 9 Desktop Runtime on
    # the far machine. It launches on the machine that built it either way, so the gap is invisible
    # exactly where it is checked and visible only to whoever received the copy.
    Step "publish" {
        dotnet publish (Join-Path $root 'apps\windows\ServerGlass\ServerGlass.csproj') `
            -c $configuration --nologo "-p:SgFfiDll=$dll" -p:SelfContained=true $versionArgument
    }
} else {
    Step "app" {
        dotnet build (Join-Path $root 'apps\windows\ServerGlass\ServerGlass.csproj') `
            -c $configuration --nologo "-p:SgFfiDll=$dll" $versionArgument
    }
}

# `dotnet publish` writes a `publish` subdirectory rather than replacing the build output, so the
# two forms of this script do not print the same path. Getting this wrong sends whoever ran
# -Publish to a folder that exists, contains an executable, and is not the one they just made.
$output = Join-Path $root "apps\windows\ServerGlass\bin\$configuration\net9.0-windows10.0.19041.0\win-x64"
if ($Publish) {
    $output = Join-Path $output 'publish'
}

Write-Host ""
Write-Host "Built." -ForegroundColor Green
Write-Host "  $output\ServerGlass.exe"
