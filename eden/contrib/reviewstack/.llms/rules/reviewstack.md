---
oncalls: ["scm_client_infra"]
apply_to_regex: ".*\\.(tsx?|graphql|css)$"
---

# ReviewStack Codebase Guide

ReviewStack is a novel user interface for GitHub pull requests with custom support for **stacked changes**. It runs entirely client-side with no server component (uses Netlify OAuth for GitHub authentication). The hosted instance is at [https://reviewstack.dev/](https://reviewstack.dev/).

## Project Overview

- **Purpose**: Alternative GitHub PR review UI inspired by Meta's internal code review tool
- **Design System**: Uses [GitHub's Primer design system](https://primer.style/)
- **State Management**: [Jotai](https://jotai.org/) for React state management
- **API**: GitHub GraphQL API v4 with IndexedDB caching layer
- **Syntax Highlighting**: TextMate grammars via `vscode-textmate`
- **Created with**: Create React App

## Directory Structure

```
src/
├── github/                # GitHub API clients and types
│   ├── GitHubClient.ts    # Interface defining GitHub client methods
│   ├── GraphQLGitHubClient.ts  # GraphQL implementation
│   ├── CachingGitHubClient.ts  # IndexedDB caching decorator
│   ├── gitHubCredentials.ts    # createGraphQLEndpointForHostname utility
│   ├── types.ts           # GitHub entity types (Commit, Tree, Blob, etc.)
│   ├── pullRequestTimelineTypes.ts
│   └── diffTypes.ts
├── queries/               # GraphQL query definitions (.graphql files)
├── mutations/             # GraphQL mutation definitions (.graphql files)
├── generated/             # Auto-generated code (GraphQL types, TextMate manifests)
│   ├── graphql.ts         # Generated TypeScript from GraphQL schema
│   └── textmate/          # Generated grammar manifests
├── textmate/              # TextMate grammar loading utilities
├── App.tsx                # Root component (assumes Provider + ThemeProvider ancestors)
├── jotai/                 # Jotai atoms (main state management)
│   ├── atoms.ts           # All Jotai atoms (credentials, theme, stacks, etc.)
│   ├── index.ts           # Public exports (barrel file)
│   └── hooks/             # Custom hooks for Jotai state
├── stackState.ts          # State for stacked PRs (Sapling & ghstack support)
├── saplingStack.ts        # Sapling stack body parsing
├── ghstackUtils.ts        # ghstack body parsing
├── themeState.ts          # Theme/color mode state management
├── LoginDialog.tsx        # Login UI with auth error display
├── UnauthorizedErrorHandler.tsx  # Handles 401 errors, clears token
├── diffServiceWorker.ts   # SharedWorker for diff computation
├── diffServiceClient.ts   # Client for communicating with diff worker
└── [Component].tsx        # React components
```

## Key Architectural Patterns

### State Management with Jotai

All global state is managed via Jotai atoms in `jotai/atoms.ts`:

```typescript
import {atom} from 'jotai';
import {atomFamily} from 'jotai-family';

// Primitive atoms hold state
export const gitHubOrgAndRepoAtom = atom<{org: string; repo: string} | null>(null);

// Derived atoms compute values from other atoms
export const gitHubPullRequestAtom = atom(async (get) => {
  const client = get(gitHubClientAtom);
  const id = get(gitHubPullRequestIDAtom);
  return client?.getPullRequest(id) ?? null;
});

// atomFamily for parameterized atoms
export const gitHubCommitAtom = atomFamily((oid: string) =>
  atom(async (get) => {
    const client = get(gitHubClientAtom);
    return client?.getCommit(oid) ?? null;
  })
);
```

Use `useAtomValue()` for reading, `useAtom()` for read+write, and `useSetAtom()` for updates. For async atoms where you want to handle loading states without Suspense, use the `loadable()` utility from `jotai/utils`.

### GitHub Client Architecture

The client uses a **decorator pattern** for caching:

1. `GitHubClient` - Interface defining all GitHub operations
2. `GraphQLGitHubClient` - Makes actual API calls via GraphQL
3. `CachingGitHubClient` - Wraps GraphQLGitHubClient, caches in IndexedDB

```typescript
const client = new GraphQLGitHubClient(hostname, org, repo, token);
const cachingClient = new CachingGitHubClient(db, client, org, repo);
```

IndexedDB stores: `commit`, `tree`, `blob`, `pr-fragment`

### GraphQL Code Generation

GraphQL queries/mutations in `queries/` and `mutations/` are processed by `@graphql-codegen`:

```bash
yarn graphql  # Generates src/generated/graphql.ts
```

Generated types follow pattern: `{QueryName}Data`, `{QueryName}Variables`

### Component Patterns

Components follow these conventions:

1. **Functional components** with hooks
2. **`React.memo()`** for performance-sensitive components
3. **Primer React components** (`Box`, `Text`, `Button`, etc.) for UI
4. **CSS modules** (`.css` files alongside components)

```typescript
export default function PullRequestLayout({
  org,
  repo,
  number,
}: {
  org: string;
  repo: string;
  number: number;
}): React.ReactElement {
  const setOrgAndRepo = useSetAtom(gitHubOrgAndRepoAtom);
  // ...
}
```

### Diff/Syntax Highlighting Worker

Heavy computation runs in a `SharedWorker` (`diffServiceWorker.ts`):

- Diff computation (`structuredPatch` from `diff` library)
- Syntax tokenization (TextMate grammars)
- Line range fetching for collapsed regions

Client communicates via message passing (`diffServiceClient.ts`).

### Stacked Changes Support

ReviewStack detects and displays PR stacks from two sources:

1. **Sapling** - Parses `[//]: # (BEGIN SAPLING FOOTER)` markers in PR body
2. **ghstack** - Parses `Stack from [ghstack]` prefix in PR body

See `saplingStack.ts` and `ghstackUtils.ts` for parsing logic.

## Code Generation

Run before starting development:

```bash
yarn codegen  # Runs both graphql and textmate codegen
```

Individual generators:

```bash
yarn graphql   # GraphQL types
yarn textmate  # TextMate grammar manifest
```

## Local Development

```bash
# From eden/contrib/reviewstack
yarn install
yarn codegen

# From eden/contrib/reviewstack.dev
yarn start  # Runs on http://localhost:3000/
```

**Security Note**: The dev server stores GitHub token in localStorage. Always logout before running other apps on port 3000.

## Key Types

### GitHub Types (`github/types.ts`)

```typescript
type GitObjectID = string;  // Git SHA hex string
type ID = string;           // GitHub GraphQL ID
type DateTime = string;     // ISO-8601 date string

interface Commit extends Node, GitObject {
  committedDate: DateTime;
  message: string;
  parents: GitObjectID[];
  tree: Tree;
}

interface Version {
  headCommit: GitObjectID;
  commits: VersionCommit[];
}
```

### Pull Request Timeline (`pullRequestTimelineTypes.ts`)

Timeline items are discriminated unions with `__typename`:

```typescript
type TimelineItem =
  | PullRequestCommit
  | HeadRefForcePushedEvent
  | PullRequestReview
  | IssueComment
  // ...
```

## Testing

```bash
yarn test  # Jest with react-scripts
```

Test files: `*.test.ts` alongside source files.

## UI Components Reference

| Component | Purpose |
|-----------|---------|
| `PullRequestLayout` | Main PR view with timeline drawer |
| `SplitDiffView` | Side-by-side diff viewer |
| `PullRequestStack` | Displays stacked PRs |
| `PullRequestTimeline` | PR activity feed |
| `InlineCommentThread` | Diff inline comments |
| `PullRequestVersionSelector` | Version/commit history selector |
| `LoginDialog` | Login form with auth error display |
| `UnauthorizedErrorHandler` | Handles expired/revoked token errors |

## Common Patterns to Follow

1. **Error handling**: Use `ErrorBoundary` component for React error boundaries
2. **Auth error handling**: Use `UnauthorizedErrorHandler` for 401 errors; it sets `authErrorMessageAtom` and clears the token
3. **Loading states**: Use `CenteredSpinner` with descriptive message
4. **Date formatting**: Use `formatISODate()` from `utils.ts`
5. **Short commit hashes**: Use `shortOid()` from `utils.ts`
6. **Grouping data**: Use `groupBy()` or `groupByDiffSide()` utilities
7. **Path joining**: Use `joinPath()` (handles null base paths)

## Styling Guidelines

- Use Primer React components and their `sx` prop for styles
- CSS files named same as component (e.g., `SplitDiffView.css`)
- Theme colors via Primer tokens: `canvas.default`, `fg.muted`, `border.default`, etc.
- Light/dark mode handled via `primerColorModeAtom` atom
