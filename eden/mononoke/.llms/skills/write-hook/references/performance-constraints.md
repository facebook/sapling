# Hook Performance Constraints

Hooks run on every push in the critical path. **P95 latency must be under 2 seconds.** All guidance below exists to meet this budget:


- **Filter early**: check `changeset.file_changes()` before deriving manifests. If no files match, return `Accepted` immediately.
- **Never walk the entire tree.** Only look up directories/files touched by the changeset.
- **Skip non-changes**: filter `fc.is_changed()` -- deletions don't grow directories.
- **Use ContentManifest rollup data** for recursive directory size/count -- O(1) per directory. Requires `content_manifest_derivation` dep and `ADDITIONAL_DERIVED_DATA="content_manifests"` in integration tests.
- **Dedupe before fan-out**: collect the touched directories into a `HashSet` keyed on their path components,
  then check them with `buffer_unordered` (see `depth2_paths` in `limit_users_directory_size.rs`) -- many files
  in one directory then cost one manifest lookup.
