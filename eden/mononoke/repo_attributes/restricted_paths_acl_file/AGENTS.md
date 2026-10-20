# restricted_paths_acl_file

Parser for `.slacl` files, the in-repo declaration of a Path ACL restriction
(a tent). A directory with a `.slacl` is a restriction root. Design overview:
`repo_attributes/restricted_paths/AGENTS.md`.

## Format

TOML, deserialized into the thrift struct `RawRestrictedPathsAclFile`, then
validated into `metaconfig_types::RestrictedPathsAclFile`:

```toml
version = 0                                              # optional; only 0 is supported
repo_region_acl = "REPO_REGION:repos/hg/<repo>/=<name>"  # required
permission_request_group = "GROUP:<group>"               # optional
rollout_allowlist_group = "<group>"                      # optional, bare group name
```

- `repo_region_acl` must be a `REPO_REGION:` identity
  (`RestrictedPathsAclFile::validate`). Callers need `read` on it to read the
  tent and `maintainers` on it to change the `.slacl`.
- `permission_request_group`: the identity a denied caller is told to request.
- `rollout_allowlist_group`: a **bare** group name, turned into `GROUP:<name>`
  by `parse_bare_group_name`, the same spelling the repo config uses. Empty,
  whitespace-padded, or any value containing `:` is rejected: accepting it
  would create an identity that never matches any caller.

## Rules for changes

- The format is a compatibility contract with files already committed to
  repos. Add optional fields only; anything else needs a new `version`.
- Parsing is strict.
- A new field must flow through `RestrictedPathsAclFile`, the AclManifest
  entry blob (`mononoke_types::acl_manifest::AclManifestEntryBlob`) and the
  restriction info types. Unset optional fields must not change the entry
  blob's encoding, or every derived AclManifest changes id.
- Keep `.slacl` and config (`PathRestrictionMetadata`) semantics identical for
  shared fields.

## Test

```bash
buck2 test @fbcode//mode/opt fbcode//eden/mononoke/repo_attributes/restricted_paths_acl_file:restricted_paths_acl_file
```

Update this file when the format or validation rules change.
