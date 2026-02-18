# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Sapling SCM is a cross-platform, highly scalable, Git-compatible source control system. It consists of three main components:

- **Sapling CLI** (`eden/scm/`): The `sl` command-line tool, based on Mercurial. Written in Python and Rust.
- **Interactive Smartlog (ISL)** (`addons/`): Web-based and VS Code UI for repository visualization. React/TypeScript.
- **Mononoke** (`eden/mononoke/`): Server-side distributed source control server (Rust). Not yet publicly supported.
- **EdenFS** (`eden/fs/`): Virtual filesystem for large checkouts (C++). Not yet publicly supported.

## Build Commands

### Sapling CLI (primary development)

From `eden/scm/`:

```bash
make oss              # Build OSS version for local usage (creates ./sl binary)
make install-oss      # Install CLI to $PREFIX/bin
make local            # Build for inplace usage
make clean            # Remove build artifacts
```

### Running Tests

```bash
# All tests (from eden/scm/)
make tests

# Single test
make test-foo         # Runs test-foo.t

# Using getdeps (full CI-style testing)
./build/fbcode_builder/getdeps.py test --allow-system-packages --src-dir=. sapling

# Single test with getdeps
./build/fbcode_builder/getdeps.py test --allow-system-packages --src-dir=. sapling --retry 0 --filter test-foo.t
```

### ISL (Interactive Smartlog)

From `addons/`:

```bash
yarn install          # Install dependencies
yarn dev              # Start dev server for ISL
```

From `addons/isl/`:

```bash
yarn start            # Start Vite dev server
yarn build            # Production build
yarn test             # Run Jest tests
yarn eslint           # Lint TypeScript/React code
```

### Building ISL for `sl web` (IMPORTANT after ISL changes)

The `sl web` command serves ISL from a pre-built `isl-dist.tar.xz`. After making changes
to ISL code in `addons/`, you **must rebuild the tar** for `sl web` to pick them up.

**Rebuild steps** (from `addons/`):

```bash
yarn install
python3 build-tar.py -o ../eden/lib/isl-dist.tar.xz
```

This builds the client (Vite) and server (rollup), then packages everything into a tar at
`eden/lib/isl-dist.tar.xz` — where the `sl` binary automatically finds it.

After rebuilding, `sl web` in **any local repo** will serve the fork's ISL.

**How `sl web` finds the tar** (in `eden/scm/sapling/commands/isl.py`):
1. Config: `[web] isl-dist-path` in `.sl/config` or `~/.sapling/sapling.conf`
2. `../lib/isl-dist.tar.xz` relative to the `sl` binary
3. `isl-dist.tar.xz` in the `sl` binary's directory

### ISL Local Development with Hot Reload

For active development with instant feedback (no rebuild needed):

```bash
# Step 1: build ISL server (one-time, re-run if server code changes)
cd addons/isl-server && yarn build

# Step 2: start Vite dev server (hot reload on port 3000)
cd addons/isl && yarn start

# Step 3: launch ISL server pointing at your repo
cd addons/isl-server && yarn serve --dev --foreground --force --cwd ~/your-repo
```

Steps 2 and 3 run in separate terminals. Changes to ISL React code reflect instantly in the browser.

**Note:** `sl web --dev` does NOT work with this repo layout — it resolves the addons path
relative to the `sl` binary incorrectly (`eden/addons/` instead of `addons/`). Use the
`yarn serve` approach above instead.

### Website

From `website/`:

```bash
yarn start            # Start Docusaurus dev server
yarn build            # Build static site
yarn lint             # Run ESLint and Stylelint
yarn format:diff      # Check formatting with Prettier
yarn typecheck        # TypeScript type checking
```

## Architecture

### Sapling CLI (`eden/scm/`)

- `sapling/` - Python CLI modules and commands
- `lib/` - Rust libraries (117+ modules): dag, indexedlog, gitcompat, checkout, clone, etc.
- `exec/` - Rust executables: hgmain, scm_daemon
- `tests/` - `.t` test files (1000+)

### ISL (`addons/`)

Yarn workspace with:
- `isl/` - React/Vite web UI
- `isl-server/` - Node.js backend
- `vscode/` - VS Code extension
- `shared/` - Shared utilities
- `components/` - Reusable UI components

## Code Style

- 2 spaces indentation
- 80 character line length
- Rust: 2024 edition, rustfmt configured in `.rustfmt.toml`
- TypeScript/JavaScript: ESLint + Prettier
- Python: Flake8

## Dependencies

- Python 3.8+
- Rust (stable)
- CMake 3.8+
- OpenSSL
- Node.js + Yarn 1.22 (for ISL/addons)

## Git Workflow

- Branch from `main`
- Developed internally at Meta, exported to GitHub
- CLA required for contributions (https://code.facebook.com/cla)
