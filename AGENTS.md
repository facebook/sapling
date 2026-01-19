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
