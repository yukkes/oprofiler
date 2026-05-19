# Contributing

## Development Checks

Run these before opening a pull request:

```powershell
cargo fmt --check
cargo test
mvn -q -f java-agent/pom.xml package
```

For packaging changes, also run:

```powershell
./tools/package-release.ps1 -Archive
```

## Code Style

- Keep source comments in English.
- Keep UI behavior stable unless the issue explicitly asks for a behavior change.
- Prefer small modules with clear ownership over broad utility files.
- Add focused tests around protocol conversion, key handling, and agent command behavior.

## Generated Files

Do not commit `target/`, `dist/`, generated jars, expanded dependency folders, or local IDE state.
