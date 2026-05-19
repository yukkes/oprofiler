# Release Process

Release artifacts are created by `tools/package-release.ps1`.

## Local Release Build

```powershell
./tools/package-release.ps1 -Archive
```

The default target is selected from the host OS:

- Windows: `windows-x86_64`
- Linux: `linux-x86_64`
- macOS Apple Silicon: `macos-aarch64`

You can override it:

```powershell
./tools/package-release.ps1 -Target windows-x86_64 -Archive
```

## GitHub Release

Push a tag named `v*` to run `.github/workflows/release.yml`.

The workflow builds:

- `windows-x86_64`
- `linux-x86_64`
- `macos-aarch64`

Each artifact contains the exact files needed to run the TUI and attach to a JVM.
