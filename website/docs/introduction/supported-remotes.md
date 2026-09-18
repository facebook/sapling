---
sidebar_label: Supported Remotes
title: Supported Remotes
---

import {Command} from '@site/elements'

One of the most frequently asked questions from new Sapling users is: **"Which remote repositories can I use with Sapling?"** This page gives a clear, concise answer.

## Quick Reference

| Remote Type | Supported | Notes |
|---|---|---|
| **Git remotes** (any) | ✅ Yes | First-class support via `.git` mode or native `sl clone` |
| **GitHub** | ✅ Yes | Enhanced workflow: stacked PRs, `sl pr`, ISL integration |
| **GitLab** | ⚠️ Partial | Push/pull works; PR/MR integration not yet available |
| **Bitbucket** | ⚠️ Partial | Push/pull works; no dedicated review integration |
| **Gitea / Forgejo** | ⚠️ Partial | Push/pull works; no dedicated review integration |
| **Mercurial remotes** | ❌ No | Not supported in the open-source release |
| **Mononoke (Meta-internal)** | ❌ Not for OSS | Meta's production server; not available publicly |
| **Sapling-native server** | ❌ Not available | No OSS Sapling server exists at this time |

## Using Sapling with Git Remotes

Sapling works with any standard Git remote over HTTPS or SSH. There are two modes of operation:

### `.git` mode (recommended for existing repos)

If you have an existing Git repository, Sapling can work inside it without creating a separate `.sl` directory:

```bash
# Clone with git first (or use an existing clone)
git clone https://github.com/your-org/your-repo
cd your-repo

# Sapling works directly — no migration needed
sl log
sl goto main
```

In `.git` mode, Sapling stores its metadata inside `.git/sl`. Your repo stays fully compatible with Git and your colleagues who use Git natively.

### Native `sl` mode

When you clone directly with Sapling, it creates an `.sl` directory instead:

```bash
sl clone https://github.com/your-org/your-repo
```

This mode gives Sapling full control over the storage layer, but the repo is still compatible with Git remotes. You can still `sl push` and `sl pull` against any Git server.

## GitHub Integration

Sapling has the richest integration with GitHub. When working with a GitHub remote, you get access to:

- **`sl pr`** — submit stacked pull requests directly from the CLI
- **ISL (Interactive Smartlog)** — visual commit graph with inline PR status
- **`sl ghstack`** — advanced stacked PR workflow
- **Automatic PR linking** — Sapling tracks which commits correspond to which PRs

To enable GitHub features, authenticate via the GitHub CLI (`gh auth login`) or set a personal access token:

```bash
gh auth login
# or
export GITHUB_TOKEN=ghp_yourtoken
```

## Other Git Hosts (GitLab, Bitbucket, Gitea)

For non-GitHub Git hosts, basic push/pull and all local Sapling features work out of the box. The GitHub-specific review integrations (`sl pr`, ISL PR status, `sl ghstack`) are not available for these hosts.

Contributions to add review integrations for other Git providers are welcome — see issue [#1148](https://github.com/facebook/sapling/issues/1148) for the proposed approach.

## Why No Mercurial or Mononoke?

While Sapling was originally built on Mercurial internals at Meta and uses Mononoke as its production server, **neither is available in the open-source release**:

- **Mercurial remotes**: The OSS release does not include the Mercurial wire protocol client needed to pull from `hg.sr.ht`, Bitbucket Mercurial repos, or similar. Sapling's Mercurial heritage is internal only.
- **Mononoke**: Meta's Sapling server is not open-sourced and has no publicly hosted instance.

For OSS users, the practical answer is: **Sapling is primarily a better client for Git repositories.**

## Summary

If you're an open-source Sapling user:

- Use any standard Git remote (GitHub, GitLab, Bitbucket, self-hosted Gitea, etc.)
- Get the best experience on **GitHub** with full PR and ISL integration
- Collaborate seamlessly with teammates who use vanilla Git
