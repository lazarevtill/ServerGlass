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
# Two details that are not incidental:
#
#   - It generates to a temporary file. An earlier version regenerated in place and compared
#     hashes, which meant a *check* left the tree modified whenever it failed.
#   - It compares with line endings normalised. The generator writes LF; git hands the working copy
#     CRLF wherever core.autocrlf is on, which is the Windows default. Comparing bytes therefore
#     failed on every fresh checkout while passing on the machine that had just run the generator —
#     green locally, red in CI, for a difference that means nothing.
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

$temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("sg-bindings-" + [guid]::NewGuid().ToString('N') + ".cs")

try {
    cargo run --quiet -p sg-bindgen --bin csharp-bindgen -- $temporary
    if ($LASTEXITCODE -ne 0) {
        Write-Host "The binding generator failed." -ForegroundColor Red
        exit 1
    }

    function Normalised($path) {
        [System.IO.File]::ReadAllText($path).Replace("`r`n", "`n").TrimEnd("`n")
    }

    if ((Normalised $committed) -ne (Normalised $temporary)) {
        Write-Host "The committed C# bindings no longer match crates/sg-ffi/src/cabi.rs." -ForegroundColor Red
        Write-Host "Regenerate and commit them:"
        Write-Host "  cargo run -p sg-bindgen --bin csharp-bindgen"
        exit 1
    }
}
finally {
    Remove-Item $temporary -ErrorAction SilentlyContinue
}

Write-Host "The C# bindings match crates/sg-ffi/src/cabi.rs." -ForegroundColor Green
exit 0
