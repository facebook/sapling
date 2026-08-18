---
name: no-new-edenapi-names
description: "Prevent new code from using the old 'edenapi' naming -- it has been renamed to 'slapi'"
metadata:
  oncalls: ['source_control_server']
  strict: true
  apply_to_path: 'eden/mononoke/servers/slapi/.*\.rs$'
  apply_to_clients: ['code_review']
---

# No New EdenAPI Names

EdenAPI has been renamed to SaplingRemoteAPI / SLAPI. Existing `edenapi`
references are kept only for backwards compatibility.

**Do not introduce new structs, methods, functions, modules, types, or
variables named `edenapi` or `EdenApi`.** Use `slapi` / `Slapi` /
`SaplingRemoteApi` instead.

Existing `edenapi` names in this crate are legacy. Do not extend them --
add new functionality under the `slapi` naming.
