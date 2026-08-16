# Agent instructions

The working rules for this repository live in **[CLAUDE.md](CLAUDE.md)** — the invariants,
the one-round-trip design claim, how to verify a change on each platform, and a list of the
mistakes this project has actually made. Read it before editing.

On Windows, read **[docs/WINDOWS.md](docs/WINDOWS.md)** as well: setup, what can and cannot be
verified from a Windows machine, and the platform traps that have already caused real bugs.

One command verifies everything CI verifies, on either platform:

```bash
./scripts/check.sh          # macOS or Linux
.\scripts\check.ps1 -All    # Windows, including the WinUI app
```

Both files are the same instructions for any agent, whichever tool is driving.

Opening a pull request rather than committing directly? [CONTRIBUTING.md](CONTRIBUTING.md) is the
human-facing version of the same rules, and is what a reviewer will hold the change to.
