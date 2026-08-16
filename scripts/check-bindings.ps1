# Prove the committed C# bindings match the Rust they were generated from.
#
#   .\scripts\check-bindings.ps1
#
# apps/windows/ServerGlass.Core/Generated/NativeMethods.g.cs is committed, exactly as the generated
# Swift bindings under apps/shared/ServerGlassFFI/generated are, so a checkout builds without
# running a generator first. The cost of committing generated code is that it can go stale: someone
# edits crates/sg-ffi/src/cabi.rs, does not regenerate, and the C# side keeps calling a signature
# that no longer exists. That is a crash at runtime and nothing at build time.
#
# So this regenerates into a temporary directory and compares. It changes nothing.
#
# UTF-8 with a BOM, for the reason spelled out at the top of check.ps1.

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$committed = Join-Path $root 'apps\windows\ServerGlass.Core\Generated\NativeMethods.g.cs'

if (-not (Test-Path $committed)) {
    Write-Host "The generated bindings are missing entirely: $committed" -ForegroundColor Red
    Write-Host "Regenerate them: cargo run -p sg-bindgen --bin csharp-bindgen"
    exit 1
}

$before = Get-FileHash $committed -Algorithm SHA256

cargo run --quiet -p sg-bindgen --bin csharp-bindgen
if ($LASTEXITCODE -ne 0) {
    Write-Host "The binding generator failed." -ForegroundColor Red
    exit 1
}

$after = Get-FileHash $committed -Algorithm SHA256

if ($before.Hash -ne $after.Hash) {
    Write-Host "The committed C# bindings are stale." -ForegroundColor Red
    Write-Host "They have just been regenerated in place; commit the change."
    Write-Host "  git diff -- apps/windows/ServerGlass.Core/Generated/NativeMethods.g.cs"
    exit 1
}

Write-Host "The C# bindings match crates/sg-ffi/src/cabi.rs." -ForegroundColor Green
exit 0
