# Everything CI checks, in one command.
#
#   pwsh scripts/check.ps1
#
# This is the same four steps `rust:windows`, `rust:fmt`, `rust:clippy` and `rust:test` run, in the
# same order, so a green run here means a green pipeline. It exists because "the tests pass" says
# nothing about fmt and clippy, and this repository has been left red for eight commits over
# exactly that.
#
# It builds and tests the Rust core, which is the whole of what Windows can verify. The macOS, iOS
# and Android apps need their own toolchains — see docs/WINDOWS.md.

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

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo is not on PATH. Install Rust:" -ForegroundColor Red
    Write-Host "  winget install Rustlang.Rustup; rustup default 1.89.0"
    Write-Host "You also need the MSVC C++ build tools — aws-lc-sys compiles C during the build:"
    Write-Host "  winget install Microsoft.VisualStudio.2022.BuildTools"
    exit 1
}

rustc --version
cargo --version

Step "format" { cargo fmt --all -- --check }
Step "clippy"  { cargo clippy --workspace --all-targets --locked -- -D warnings }
Step "build"   { cargo build --workspace --locked }
Step "test"    { cargo test --workspace --locked }

Write-Host ""
Write-Host "All checks passed." -ForegroundColor Green
Write-Host "The live SSH tests skipped themselves — they need the Docker fixtures." -ForegroundColor DarkGray
Write-Host "To run them too: wsl ./fixtures/up.sh; `$env:SG_REQUIRE_FIXTURES='1'; cargo test --workspace" -ForegroundColor DarkGray
