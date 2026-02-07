# Jotai State Management

This directory contains Jotai atoms for the ReviewStack application.

## Architecture

The app uses [Jotai](https://jotai.org/) for React state management. All state is defined in `atoms.ts` and exported through `index.ts`.

### Key Atom Categories

- **Theme**: `primerColorModeAtom` - User's color mode preference (day/night)
- **Credentials**: `gitHubTokenPersistenceAtom`, `gitHubTokenStateAtom`, `gitHubHostnameAtom`, `gitHubUsernameAtom` - Authentication with cross-tab logout support
- **GitHub Client**: `gitHubClientAtom` - Cached GitHub API client
- **Pull Request**: `gitHubPullRequestAtom`, `gitHubPullRequestForParamsAtom` - PR data and loading
- **Versions**: `gitHubPullRequestVersionsAtom`, `gitHubPullRequestSelectedVersionIndexAtom` - PR version history
- **Diffs**: `gitHubDiffCommitIDsAtom`, `gitHubPullRequestVersionDiffAtom` - Diff computation
- **Threads**: `gitHubPullRequestReviewThreadsAtom`, `gitHubThreadsForDiffFileAtom` - Review comments
- **Stacked PRs**: `stackedPullRequestAtom`, `stackedPullRequestFragmentsAtom` - PR stack support

### Diff Service Atoms (`diffServiceClient.ts`)

Heavy computation runs in a SharedWorker:

- `diffAndTokenizeAtom` - Tokenizes and diffs file contents
- `colorMapAtom` - TextMate color maps for syntax highlighting
- `lineRangeAtom` - Fetches line ranges for expanding collapsed sections
- `lineToPositionAtom` - Line to position mapping for comments

## Usage

```typescript
import {useAtom, useAtomValue, useSetAtom} from 'jotai';
import {gitHubPullRequestAtom, primerColorModeAtom} from './jotai';

// Read-only
const pullRequest = useAtomValue(gitHubPullRequestAtom);

// Read and write
const [colorMode, setColorMode] = useAtom(primerColorModeAtom);

// Write-only
const setColorMode = useSetAtom(primerColorModeAtom);
```

### Async Atoms with Loadable

For async atoms where you want to avoid Suspense:

```typescript
import {useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {useMemo} from 'react';

function MyComponent() {
  const loadableAtom = useMemo(() => loadable(asyncAtom), []);
  const result = useAtomValue(loadableAtom);

  if (result.state === 'loading') {
    return <Spinner />;
  }
  if (result.state === 'hasError') {
    return <Error error={result.error} />;
  }
  return <div>{result.data}</div>;
}
```

## API Quick Reference

| Hook | Purpose |
|------|---------|
| `useAtom(atom)` | Read and write an atom |
| `useAtomValue(atom)` | Read-only access to an atom |
| `useSetAtom(atom)` | Write-only access to an atom |
| `useStore()` | Access the Jotai store directly |

| Utility | Purpose |
|---------|---------|
| `atom()` | Create a primitive or derived atom |
| `atomFamily()` | Create parameterized atoms (from `jotai-family`) |
| `atomWithStorage()` | Create an atom with localStorage persistence |
| `loadable()` | Wrap async atom to get loading/error/data states |
