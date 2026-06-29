---
name: write-hook
description: Create a Mononoke server-side hook with 3-diff split (tests, wiring, implementation), unit tests, and integration tests
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/features/hooks/src/(facebook/)?implementations/.*\.rs$'
  apply_to_user_prompt: '.*(write|create|add|new|implement).*(mononoke\s+)?hook.*'
---

# Write a Mononoke Hook

Create a server-side Mononoke hook based on the user's request.

- When creating a new hook, read [references/implementation-guide.md](references/implementation-guide.md)
- When writing hook logic that touches manifests or derived data, read [references/performance-constraints.md](references/performance-constraints.md)
- When writing unit or integration tests, read [references/testing-patterns.md](references/testing-patterns.md)
- Before submitting, read [references/common-mistakes.md](references/common-mistakes.md)
- After implementation is complete, read [references/configerator-setup.md](references/configerator-setup.md) for enabling the hook in production
