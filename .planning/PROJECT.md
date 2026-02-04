# Sapling ISL Fork

## What This Is

A fork of Sapling's Interactive Smartlog (ISL) web UI with Graphite-inspired polish for stacked PR workflows. Built for a small team (3-5 devs) who switched from Graphite and wanted that level of refinement in their daily code review tool.

## Core Value

The UI should feel polished and effortless — you focus on the code, not fighting the interface.

## Current State

**Version:** v1.0 (shipped 2026-01-22)
**Tech:** React, TypeScript, Vite, CSS custom properties
**LOC:** +630 lines added across 18 files

Shipped features:
- Graphite-inspired color scheme (deep navy, soft blue accents)
- Click-to-checkout navigation (single-click on any PR/commit)
- Origin/main visual prominence (badge highlighting)
- Auto-scroll sync with VS Code-style selection borders
- Author avatars with deterministic colors
- Soft diff colors (cyan-blue additions, salmon deletions)

## Current Milestone: v1.2 PR Review View

**Goal:** Add a dedicated PR review interface that transforms the three-column layout into a focused code review experience

**Target features:**
- Review mode entry (button on PR rows, sidebar action)
- File list with "viewed" checkmarks (persisted, resets on PR update)
- Full diff view with file-by-file navigation
- Inline comment system (pending comments until submission)
- Review submission (approve, request changes, comment)

## Requirements

### Validated

- Responsive three-column layout with graceful collapse — v1.0
- Proper spacing and padding throughout — v1.0
- Click commits/PRs to checkout (not just pull button) — v1.0
- "main" at top with pull/checkout button — v1.0
- Better highlighting of origin/main — v1.0
- Auto-scroll to selected commit when selecting from left — v1.0
- Show author name + avatar on commits — v1.0
- De-emphasize "changes to amend" (collapsible) — v1.0
- Promote files changed section — v1.0
- Diff view: Graphite-style color scheme — v1.0
- Overall visual polish matching Graphite's aesthetic — v1.0

### Active

- [ ] PR Review mode entry point (button on PR rows)
- [ ] File list column with "viewed" checkmarks
- [ ] Full diff view for focused file review
- [ ] Inline comment system for code review
- [ ] Review submission (approve, request changes, comment)

### Out of Scope

- Upstream contribution — this is an internal fork
- New stack review features — focus on UI polish first, discover pain points through usage
- Mobile support — desktop/laptop is the target

## Context

The team previously used Graphite and is accustomed to its polished review UI. v1.0 addresses the initial friction points:
- Three-column layout cramping on smaller screens
- Navigation requiring hunting for small buttons
- Diff colors feeling off compared to Graphite's softer palette
- Information density high without enough breathing room

## Constraints

- **Tech stack**: Work within existing ISL stack (React, TypeScript, Vite, existing CSS approach)
- **Users**: Internal team only, no need for backwards compatibility
- **Scope**: UI/UX improvements, not architectural changes to ISL

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Fork ISL rather than upstream PR | Internal team needs, faster iteration, opinionated changes | Good |
| Match Graphite's diff color scheme | Team is used to it, softer on eyes for long sessions | Good |
| Work within existing stack | Minimize learning curve, leverage existing patterns | Good |
| Deep navy #1a1f36 as primary background | Graphite-style, professional, easy on eyes | Good |
| Single-click checkout on entire PR row | Matches Graphite UX pattern — most common action easiest | Good |
| 12-color avatar palette with deterministic hash | Same author always gets same color for consistency | Good |

---
*Last updated: 2026-02-02 after starting v1.2 milestone*
