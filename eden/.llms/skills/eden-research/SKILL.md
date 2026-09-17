---
name: eden-research
description: >
  Locate Eden implementations and trace behavior across Sapling, EdenFS,
  and Mononoke when the relevant code path is not already known.
metadata:
  oncalls:
    - 'scm_client_infra'
    - 'scm_server_infra'
    - 'sapling'
  strict: true
  apply_to_path: 'eden/.*'
---

# Eden research

Eden spans the Sapling client, EdenFS virtual filesystem, and Mononoke server.
Use the smallest reference set that covers the question.

## Route by component

| Scope | Read when needed |
|-------|------------------|
| EdenFS inodes, stores, kernel channels, or Thrift service | [`fbcode/eden/.llms/skills/eden-research/references/edenfs.md`](references/edenfs.md) |
| Sapling commands, libraries, bindings, or working copy | [`fbcode/eden/.llms/skills/eden-research/references/sapling.md`](references/sapling.md) |
| Mononoke APIs, servers, derived data, or blobstores | [`fbcode/eden/.llms/skills/eden-research/references/mononoke.md`](references/mononoke.md) |
| Real-daemon and filesystem integration tests | `fbcode/eden/integration/.claude/CLAUDE.md` |
| Shared C++ utilities | `fbcode/eden/common/.claude/CLAUDE.md` |
| New EdenAPI or SLAPI endpoint | [`fbcode/eden/.llms/skills/CREATING_ENDPOINTS/SKILL.md`](../CREATING_ENDPOINTS/SKILL.md) |

For a cross-component flow, read only the references for the boundaries it
crosses. Consult the component's `.claude/CLAUDE.md` when implementation or
verification conventions matter.
