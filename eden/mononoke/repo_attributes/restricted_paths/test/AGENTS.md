# restricted_paths: scenario tests (`test/`)

The `restricted_paths_test` target (`BUCK`, `srcs = test/**/*.rs`) runs
end-to-end scenarios against a test repo: it creates commits, derives
manifests, fills the manifest-id store, calls the public API, and checks the
access-log rows and enforcement results. Read `../AGENTS.md` for the design
under test.

Unit tests for private helpers live in `../src/` (inline in `lib.rs`, or
`src/<module>/tests.rs`). Put a test here when it needs a repo, derived data,
or exact access-log rows.

## Files

- `main.rs`: one `#[mononoke::fbinit_test]` per behaviour.
- `utils.rs`: the harness.
  - `RestrictedPathsTestDataBuilder` describes a scenario: restrictions
    (`with_restricted_paths` for both sources, the common case, and the other
    `with_*` restriction builders), ACLs and groups
    (`with_test_repo_region_acls`, `with_test_groups`), the caller
    (`with_client_identity` and the other `with_*` caller builders), the commit
    (`with_file_path_changes`) and expectations
    (`expecting_manifest_id_store_entries`, `expecting_scuba_access_logs`).
  - `RestrictedPathsTestData::run_restricted_paths_test` and
    `run_restricted_paths_test_scenario` build a repo, run every access and
    compare rows and store entries with the expectations. `observe_*` helpers
    return raw observations for custom assertions; `build_repo_only` builds the
    repo without running accesses.
  - `ScubaAccessLogSampleBuilder` builds expected rows. Start from a shared
    base sample and add only the fields each access changes.
  - `TEST_CLIENT_MAIN_ID` is the main id logged for the default synthetic
    caller (`USER:myusername0`).

Extend the harness when a test needs a capability it lacks.

## Conventions

- New tests start with a `What it tests:` / `Expected:` comment.
- Expected manifest ids are hard-coded. If derivation changes them, confirm
  the change is intended before updating the constants.
- Cover the negative case (not restricted, authorized caller) next to the
  positive one.
- Before a behaviour change, add the scenario pinning the existing output in a
  separate diff; the behaviour diff then flips only the expectations.

## Run

```bash
buck2 test @fbcode//mode/opt fbcode//eden/mononoke/repo_attributes/restricted_paths:restricted_paths_test
```

Update this file when the harness gains or loses a capability.
