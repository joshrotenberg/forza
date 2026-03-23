# Go project context

## Language and toolchain

This is a Go project. Check `go.mod` for the module name and Go version.

## Common commands

```bash
go build ./...               # compile all packages
go test ./...                # run all tests
go test -race ./...          # run tests with race detector
go vet ./...                 # static analysis
gofmt -w .                   # format code
golangci-lint run            # lint (if golangci-lint is installed)
```

## Conventions

- Follow standard Go idioms and naming conventions (camelCase for unexported, PascalCase for exported).
- Return errors as the last return value; do not panic in library code.
- Write table-driven tests with `t.Run(name, func(t *testing.T) {...})`.
- Use `testify/assert` or `testify/require` if already in the project's dependencies.
- Do not use `init()` unless necessary.
- Keep `go.sum` committed and up to date.
- Check `.github/workflows/` for the exact test and lint commands used in CI.

## Project layout

Check `go.mod` for the module path. Follow the standard Go project layout: `cmd/` for binaries, `internal/` for private packages, `pkg/` for public packages.
