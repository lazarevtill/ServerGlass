# Build the Windows installer, and optionally install it here.
#
#   .\scripts\package-windows.ps1              dist\ServerGlass-<version>-windows-x64-setup.exe
#   .\scripts\package-windows.ps1 -Install     and install it on this machine
#   .\scripts\package-windows.ps1 -SkipBuild   package what is already in the publish folder
#
# The Windows half of scripts/release.sh, which builds the macOS .dmg and the Android .apk and
# cannot build this one: an Inno Setup installer needs a Windows machine, and release.sh runs on a
# Mac. So this is a separate command rather than another function in there.
#
# The install is per-user. It lands in %LOCALAPPDATA%\Programs\ServerGlass, needs no administrator,
# and raises no UAC prompt - see apps/windows/installer/ServerGlass.iss for why that is the right
# shape for this app rather than a limitation.
#
# UTF-8 with a BOM, for the reason spelled out at the top of check.ps1.

param(
    [switch]$Install,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$publish = Join-Path $root 'apps\windows\ServerGlass\bin\Release\net9.0-windows10.0.19041.0\win-x64\publish'
$iss = Join-Path $root 'apps\windows\installer\ServerGlass.iss'
$dist = Join-Path $root 'dist'

function Step($name, $block) {
    Write-Host ""
    Write-Host "==> $name" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $name" -ForegroundColor Red
        exit 1
    }
}

# Inno Setup installs per-user by default, which puts ISCC somewhere PATH has never heard of.
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )
    $iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) {
    Write-Host "Inno Setup is not installed. It compiles the installer:" -ForegroundColor Red
    Write-Host "  winget install JRSoftware.InnoSetup"
    exit 1
}

# One version, read from the workspace manifest. Typing it into the .iss as well is how a release
# ends up shipping an installer whose Apps & Features entry disagrees with the app's own About box.
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) {
    Write-Host "Could not read the version out of Cargo.toml." -ForegroundColor Red
    exit 1
}
Write-Host "ServerGlass $version" -ForegroundColor Green

if (-not $SkipBuild) {
    Step "app (release, self-contained)" {
        & "$PSScriptRoot\build-windows.ps1" -Release -Publish -Version $version
    }
}

if (-not (Test-Path (Join-Path $publish 'ServerGlass.exe'))) {
    Write-Host "There is nothing to package: $publish\ServerGlass.exe is missing." -ForegroundColor Red
    Write-Host "Run without -SkipBuild."
    exit 1
}

# Assertions about the payload. Every one of these is a file whose absence produces an installer
# that builds, installs and then fails - on this machine or on the next one.
#
#   sg_ffi.dll - the app is a view layer around the Rust core and does nothing without it. It gets
#   here by a csproj copy rule that is conditional on the file existing, so a stale or missing
#   build silently ships an app that cannot read a single metric.
#
#   System.Private.CoreLib.dll - only a self-contained publish has it. Without it the folder needs
#   the .NET 9 Desktop Runtime installed separately, which every developer machine already has and
#   almost no target machine does.
#
#   ServerGlass.pri and the .xbf files - the compiled XAML and its index. `dotnet publish` drops
#   both unless the csproj puts them back; see the PublishTheCompiledXaml target for why. The app
#   crashes in its own constructor without them, and the first version of this installer shipped
#   exactly that.
#
#   Assets\ServerGlass.ico - WinUI's title bar cannot read the icon embedded in the executable, so
#   the window sets its own from this file. A publish without it throws on the first line of the
#   main window rather than merely looking plain.
foreach ($required in 'sg_ffi.dll', 'System.Private.CoreLib.dll', 'ServerGlass.pri', 'Assets\ServerGlass.ico') {
    if (-not (Test-Path (Join-Path $publish $required))) {
        Write-Host "$required is missing from the publish folder, so the installer would ship an app that cannot run." -ForegroundColor Red
        exit 1
    }
}

if (-not (Get-ChildItem $publish -Filter '*.xbf' -Recurse -File)) {
    Write-Host "There is no compiled XAML (*.xbf) in the publish folder, so the app would crash on launch." -ForegroundColor Red
    exit 1
}

# The binary has to agree with the installer wrapping it. It is the version a crash report carries,
# and a support conversation that starts from the wrong number goes nowhere. This also catches
# -SkipBuild being pointed at a publish folder left over from an older version.
$stamped = (Get-Item (Join-Path $publish 'ServerGlass.exe')).VersionInfo.ProductVersion
if ($stamped -notlike "$version*") {
    Write-Host "The published executable says version $stamped, but this is $version." -ForegroundColor Red
    Write-Host "Run without -SkipBuild."
    exit 1
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
$setup = Join-Path $dist "ServerGlass-$version-windows-x64-setup.exe"

Step "installer" {
    & $iscc /Qp "/DAppVersion=$version" "/DSourceDir=$publish" "/DOutputDir=$dist" $iss
}

if (-not (Test-Path $setup)) {
    Write-Host "ISCC reported success but $setup is missing." -ForegroundColor Red
    exit 1
}

$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host ""
Write-Host "Built." -ForegroundColor Green
Write-Host "  $setup  ($size MB)"

if ($Install) {
    # VERYSILENT because this is a scripted install; a person double-clicking the same file gets the
    # ordinary wizard. No elevation is involved, so nothing here needs an administrator.
    $log = Join-Path ([System.IO.Path]::GetTempPath()) "serverglass-install.log"
    Step "install" {
        $process = Start-Process -FilePath $setup -Wait -PassThru `
            -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=$log"
        # Start-Process does not set $LASTEXITCODE, which Step reads.
        $global:LASTEXITCODE = $process.ExitCode
    }

    $installed = Join-Path $env:LOCALAPPDATA 'Programs\ServerGlass\ServerGlass.exe'
    if (-not (Test-Path $installed)) {
        Write-Host "The installer reported success but $installed is missing. Log: $log" -ForegroundColor Red
        exit 1
    }

    Write-Host ""
    Write-Host "Installed." -ForegroundColor Green
    Write-Host "  $installed"
    Write-Host "  Start Menu > ServerGlass, and Apps & Features > ServerGlass to remove it."
    Write-Host "  Your hosts and credentials live in $env:LOCALAPPDATA\ServerGlass and survive an uninstall." -ForegroundColor DarkGray
}
