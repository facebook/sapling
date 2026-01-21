# Sapling ISL Fork

## What This Is

A fork of Sapling's Interactive Smartlog (ISL) web UI, focused on improving the developer experience for stacked PR workflows. Built for a small team (3-5 devs) who switched from Graphite and want that level of polish in their daily code review tool.

## Core Value

The UI should feel polished and effortless — you focus on the code, not fighting the interface.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Responsive three-column layout with graceful collapse on narrow screens
- [ ] Proper spacing and padding throughout — no cramped feeling
- [ ] Left column: click commits/PRs to checkout (not just pull button)
- [ ] Left column: "main" at top with pull/checkout button
- [ ] Left column: better highlighting of origin/main
- [ ] Middle column: auto-scroll to selected commit when selecting from left
- [ ] Middle column: show author name + avatar on commits
- [ ] Middle column: highlight origin/main more prominently
- [ ] Right column: de-emphasize "changes to amend" (collapsible, or move below files)
- [ ] Right column: promote files changed section
- [ ] Right column: +/- line counts in GitHub/Graphite style
- [ ] Diff view: Graphite-style color scheme (soft blue additions, salmon deletions)
- [ ] Overall visual polish matching Graphite's aesthetic

### Out of Scope

- Upstream contribution — this is an internal fork
- New stack review features — focus on UI polish first, discover pain points through usage
- Mobile support — desktop/laptop is the target

## Context

The team previously used Graphite and is accustomed to its polished review UI. The current ISL works but has friction:
- Three-column layout cramps on smaller screens
- Navigation requires hunting for small buttons
- Diff colors feel off compared to Graphite's softer palette
- Information density is high without enough breathing room

Reference screenshot of Graphite's diff view provided — soft navy background, blue-tinted additions, salmon deletions, muted syntax highlighting.

## Constraints

- **Tech stack**: Work within existing ISL stack (React, TypeScript, Vite, existing CSS approach)
- **Users**: Internal team only, no need for backwards compatibility
- **Scope**: UI/UX improvements, not architectural changes to ISL

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Fork ISL rather than upstream PR | Internal team needs, faster iteration, opinionated changes | — Pending |
| Match Graphite's diff color scheme | Team is used to it, softer on eyes for long sessions | — Pending |
| Work within existing stack | Minimize learning curve, leverage existing patterns | — Pending |

---
*Last updated: 2025-01-21 after initialization*
