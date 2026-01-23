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

## Current Milestone: v1.1 Refinement & Fixes

**Goal:** Fix v1.0 issues discovered in usage and refine color scheme to match Graphite more closely

**Target improvements:**
- Fix broken auto-scroll sync between left and middle columns
- Reduce visual prominence of middle column action bar
- Fix double-scroll issue in left column
- Show +/- line counts in file change statistics
- Add configurable editor for opening files
- Match Graphite's color palette more closely (darker backgrounds, muted greens, better grays)

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

- [ ] Auto-scroll sync properly positions "you are here" commit at viewport top
- [ ] Top action bar has reduced opacity until hovered
- [ ] Left column has single scrollable area (no double-scroll)
- [ ] Files changed shows +123/-45 style line counts
- [ ] User can configure preferred editor for opening files
- [ ] Color scheme matches Graphite screenshots (darker backgrounds, muted additions, better grays)

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
*Last updated: 2026-01-23 after starting v1.1 milestone*
