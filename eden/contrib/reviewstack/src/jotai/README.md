# Recoil to Jotai Migration Guide

This directory contains Jotai atoms that are being migrated from Recoil.

## Current Migration Status

The app is currently in a hybrid state where both Recoil and Jotai providers coexist.
This allows incremental migration of atoms and their consumers.

### Fully Migrated to Jotai (in `atoms.ts`)

These atoms are now natively implemented in Jotai:

- **Theme**: `primerColorModeAtom`
- **Credentials (simple)**: `gitHubHostnameAtom`, `isConsumerGitHubAtom`, `gitHubGraphQLEndpointAtom`
- **Org/Repo**: `gitHubOrgAndRepoAtom`
- **GitHub Client**: `gitHubClientAtom`, `gitHubBlobAtom`, `gitHubCommitAtom`
- **Pull Request**: `gitHubPullRequestAtom`, `gitHubPullRequestIDAtom`, `gitHubPullRequestViewerDidAuthorAtom`, `gitHubPullRequestViewerCanUpdateAtom`
- **Labels/Reviewers**: `gitHubPullRequestLabelsAtom`, `gitHubPullRequestReviewersAtom`, `gitHubRepoLabelsQuery`, `gitHubRepoLabels`, `gitHubRepoAssignableUsersQuery`, `gitHubRepoAssignableUsers`
- **Versions**: `gitHubPullRequestVersionsAtom` (synced from Recoil), `gitHubPullRequestSelectedVersionIndexAtom`, `gitHubPullRequestComparableVersionsAtom`, `gitHubPullRequestIsViewingLatestAtom`
- **Diffs**: `gitHubDiffCommitIDsAtom`, `gitHubPullRequestVersionDiffAtom`, `gitHubDiffForCommitsAtom`, `gitHubDiffForCurrentCommitAtom`
- **Threads**: `gitHubPullRequestReviewThreadsAtom`, `gitHubThreadsForDiffFileAtom`, `gitHubPullRequestThreadsByCommitAtom`
- **Comments**: `gitHubPullRequestNewCommentInputCellAtom`, `gitHubPullRequestCanAddCommentAtom`, `gitHubPullRequestPendingReviewIDAtom`
- **Check Runs**: `gitHubPullRequestCheckRunsAtom`
- **User Home Page**: `gitHubUserHomePageDataAtom`
- **Pull Requests Search**: `gitHubPullRequestsAtom`
- **Notifications**: `notificationMessageAtom`
- **Stacked PRs**: `stackedPullRequestAtom`, `stackedPullRequestFragmentsAtom` (components use these; Recoil versions kept for gitHubPullRequestVersions)

### Migrated in `diffServiceClient.ts`

These diff service atoms are now Jotai-native:

- `diffAndTokenizeAtom` - Tokenizes and diffs file contents
- `colorMapAtom` - TextMate color maps for syntax highlighting
- `lineRangeAtom` - Fetches line ranges for expanding collapsed sections
- `lineToPositionAtom` - Line to position mapping for comments

### Still in Recoil (in `recoil.ts`)

These remain in Recoil due to complex dependencies or cross-tab effects:

- **Pull Request Loading**: `gitHubPullRequestForParams` - Used by `PullRequest.tsx` for initial loading and refresh; uses Recoil's `refresh()` API
- **Versions Computation**: `gitHubPullRequestVersions` - Complex selector with many dependencies including stack state
- **Line-to-Position**: `gitHubPullRequestLineToPositionForFile` - Complex dependency chain involving commit comparisons
- **GitHub Client (Recoil)**: `gitHubClient` - Used by `stackState.ts` and internal selectors

### Still in Recoil (in `gitHubCredentials.ts`)

Authentication atoms with complex cross-tab sync:

- `gitHubTokenPersistence` - Token with localStorage persistence and cross-tab logout
- `gitHubUsername` - Username fetching and caching
- `gitHubHostname` - Used internally by Recoil selectors (Jotai version `gitHubHostnameAtom` available for components)
- `isConsumerGitHub` - Used internally by Recoil selectors (Jotai version `isConsumerGitHubAtom` available for components)

### Still in Recoil (in `stackState.ts`)

Stacked PR support (Recoil versions kept for `gitHubPullRequestVersions`):

- `stackedPullRequest` - Detects Sapling/ghstack PR stacks (components now use `stackedPullRequestAtom`)
- `stackedPullRequestFragments` - Fetches stack PR data (components now use `stackedPullRequestFragmentsAtom`)

### Still in Recoil (in `shared/Drawers.tsx`)

The shared Drawers component uses Recoil internally:

- Accepts `RecoilState<AllDrawersState>` as prop
- Used by `PullRequestLayout.tsx`

### Sync Layer

`JotaiRecoilSync.tsx` synchronizes state between Jotai and Recoil:

- **Jotai → Recoil**: `gitHubOrgAndRepoAtom`, `gitHubPullRequestAtom`
- **Recoil → Jotai**: `gitHubPullRequestVersions`
- **Bidirectional**: `gitHubPullRequestSelectedVersionIndex`, `gitHubPullRequestComparableVersions`

## Next Steps for Full Migration

1. **Migrate `stackState.ts`** - Convert to Jotai atoms
2. **Migrate `gitHubPullRequestVersions`** - Complex but would eliminate Recoil→Jotai sync
3. **Migrate `gitHubCredentials.ts`** - Requires reimplementing cross-tab effects in Jotai
4. **Migrate `shared/Drawers.tsx`** - Update to accept Jotai atoms
5. **Migrate `gitHubPullRequestForParams`** - Need Jotai equivalent of `refresh()`

## How to Migrate an Atom

### 1. Create the Jotai Atom

For a simple Recoil atom:

```typescript
// Before (Recoil)
import {atom} from 'recoil';

export const myAtom = atom<string>({
  key: 'myAtom',
  default: 'initial value',
});

// After (Jotai)
import {atom} from 'jotai';

export const myAtom = atom<string>('initial value');
```

For a Recoil atom with effects (persistence, etc.):

```typescript
// Before (Recoil)
import {atom} from 'recoil';

export const myAtom = atom<string>({
  key: 'myAtom',
  default: 'initial value',
  effects: [localStorageEffect('myAtom')],
});

// After (Jotai)
import {atomWithStorage} from 'jotai/utils';

export const myAtom = atomWithStorage<string>('myAtom', 'initial value');
```

### 2. Migrate Selectors to Derived Atoms

```typescript
// Before (Recoil)
import {selector} from 'recoil';

export const mySelector = selector({
  key: 'mySelector',
  get: ({get}) => {
    const value = get(myAtom);
    return value.toUpperCase();
  },
});

// After (Jotai)
import {atom} from 'jotai';

export const myDerivedAtom = atom((get) => {
  const value = get(myAtom);
  return value.toUpperCase();
});
```

### 3. Migrate Async Selectors

```typescript
// Before (Recoil)
import {selector} from 'recoil';

export const asyncSelector = selector({
  key: 'asyncSelector',
  get: async ({get}) => {
    const id = get(idAtom);
    const response = await fetch(`/api/data/${id}`);
    return response.json();
  },
});

// After (Jotai)
import {atom} from 'jotai';

export const asyncAtom = atom(async (get) => {
  const id = get(idAtom);
  const response = await fetch(`/api/data/${id}`);
  return response.json();
});
```

### 4. Update Component Consumers

```typescript
// Before (Recoil)
import {useRecoilState, useRecoilValue} from 'recoil';

function MyComponent() {
  const [value, setValue] = useRecoilState(myAtom);
  const derivedValue = useRecoilValue(mySelector);
  // ...
}

// After (Jotai)
import {useAtom, useAtomValue} from 'jotai';

function MyComponent() {
  const [value, setValue] = useAtom(myAtom);
  const derivedValue = useAtomValue(myDerivedAtom);
  // ...
}
```

### 5. Handling Loadable States

```typescript
// Before (Recoil)
import {useRecoilValueLoadable} from 'recoil';

function MyComponent() {
  const loadable = useRecoilValueLoadable(asyncSelector);
  switch (loadable.state) {
    case 'hasValue':
      return <div>{loadable.contents}</div>;
    case 'loading':
      return <Spinner />;
    case 'hasError':
      return <Error error={loadable.contents} />;
  }
}

// After (Jotai)
import {useAtom} from 'jotai';
import {loadable} from 'jotai/utils';

const loadableAsyncAtom = loadable(asyncAtom);

function MyComponent() {
  const [result] = useAtom(loadableAsyncAtom);
  if (result.state === 'loading') {
    return <Spinner />;
  }
  if (result.state === 'hasError') {
    return <Error error={result.error} />;
  }
  return <div>{result.data}</div>;
}
```

## Migration Checklist

When migrating an atom/selector:

1. [ ] Create the Jotai atom in `src/jotai/atoms.ts`
2. [ ] Update all component imports to use `jotai` instead of `recoil`
3. [ ] Replace hook calls (`useRecoilState` → `useAtom`, etc.)
4. [ ] Test the component thoroughly
5. [ ] Remove the old Recoil atom from `recoil.ts` once all consumers are migrated

## Files to Update After Full Migration

Once all atoms are migrated:

1. Remove `recoil` from `package.json` dependencies
2. Remove `RecoilRoot` from `reviewstack.dev/public/index.html`
3. Remove `recoil.ts` file
4. Update `App.tsx` comments that reference RecoilRoot

## API Equivalents Quick Reference

| Recoil | Jotai |
|--------|-------|
| `atom()` | `atom()` |
| `selector()` | `atom((get) => ...)` |
| `useRecoilState()` | `useAtom()` |
| `useRecoilValue()` | `useAtomValue()` |
| `useSetRecoilState()` | `useSetAtom()` |
| `useRecoilValueLoadable()` | `useAtom(loadable(atom))` |
| `atomFamily()` | `atomFamily()` from `jotai-family` |
| `selectorFamily()` | Use `atomFamily()` with derived atom pattern |
| `waitForAll()` | Use `Promise.all()` in async atom |
