---
path: /Users/jonas/code/sapling/addons/isl-server/src/github/githubCodeReviewProvider.ts
type: service
updated: 2026-01-21
status: active
---

# githubCodeReviewProvider.ts

## Purpose

GitHub-specific code review provider implementation. Fetches PR summaries, comments, and review state from GitHub GraphQL API. Bridges ISL with GitHub pull requests.

## Exports

- `GitHubCodeReviewProvider` - GitHub provider class
- `GitHubDiffSummary` - GitHub PR data type

## Dependencies

- [[addons-isl-src-types]] - Diff and review types
- shared utilities - Type-safe event emitter, debouncing

## Used By

TBD

## Notes

Implements code review provider interface for GitHub. Handles GraphQL queries, stack parsing from PR bodies, and PR state synchronization.
