/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {WorktreeInfo} from './types';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {Column, ScrollY} from './ComponentUtils';
import {T, t} from './i18n';
import {useRunOperation} from './operationsState';
import {WorktreeRemoveOperation} from './operations/WorktreeRemoveOperation';
import {repositoryInfo} from './serverAPIState';
import {worktreesAtom} from './worktrees';
import serverAPI from './ClientToServerAPI';

const styles = stylex.create({
  container: {
    position: 'relative',
  },
  dropdownContainer: {
    width: 'max-content',
    minWidth: '280px',
    maxWidth: 'min(500px, 90vw)',
    alignItems: 'flex-start',
    padding: 'var(--pad)',
    gap: 'var(--halfpad)',
  },
  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    width: '100%',
    paddingBottom: 'var(--halfpad)',
    borderBottom: '1px solid var(--subtle-hover-darken)',
  },
  headerTitle: {
    fontWeight: 600,
    fontSize: '13px',
  },
  worktreeList: {
    width: '100%',
    display: 'flex',
    flexDirection: 'column',
    gap: '4px',
  },
  worktreeItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 'var(--halfpad)',
    padding: 'var(--halfpad)',
    borderRadius: '4px',
    cursor: 'pointer',
    width: '100%',
    boxSizing: 'border-box',
    backgroundColor: {
      default: 'transparent',
      ':hover': 'var(--hover-darken)',
    },
  },
  worktreeItemCurrent: {
    backgroundColor: {
      default: 'var(--selected-commit-background, rgba(74, 144, 226, 0.15))',
      ':hover': 'var(--selected-commit-background, rgba(74, 144, 226, 0.15))',
    },
    borderLeft: '2px solid var(--graphite-accent, #4a90e2)',
  },
  worktreeIcon: {
    flexShrink: 0,
    color: 'var(--foreground-sub)',
  },
  worktreeIconMain: {
    color: 'var(--graphite-accent, #4a90e2)',
  },
  worktreeInfo: {
    display: 'flex',
    flexDirection: 'column',
    gap: '2px',
    overflow: 'hidden',
    flex: 1,
    minWidth: 0,
  },
  worktreeName: {
    fontWeight: 500,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  worktreePath: {
    fontSize: '11px',
    color: 'var(--foreground-sub)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    fontFamily: 'monospace',
  },
  currentBadge: {
    fontSize: '10px',
    padding: '1px 4px',
    borderRadius: '3px',
    background: 'color-mix(in srgb, var(--graphite-accent, #4a90e2) 20%, transparent)',
    color: 'var(--graphite-accent, #4a90e2)',
    flexShrink: 0,
  },
  mainBadge: {
    fontSize: '10px',
    padding: '1px 4px',
    borderRadius: '3px',
    background: 'var(--subtle-hover-darken)',
    color: 'var(--foreground-sub)',
    flexShrink: 0,
  },
  deleteButton: {
    flexShrink: 0,
    opacity: {
      default: 0.5,
      ':hover': 1,
    },
    color: {
      default: 'var(--foreground-sub)',
      ':hover': 'var(--signal-medium-fg)',
    },
    cursor: 'pointer',
    padding: '2px',
    borderRadius: '3px',
    marginLeft: 'auto',
  },
  emptyState: {
    color: 'var(--foreground-sub)',
    padding: 'var(--pad)',
    textAlign: 'center',
  },
  buttonLabel: {
    maxWidth: '120px',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
});

/**
 * Worktree switcher component for the top bar.
 * Shows the current worktree and allows switching between worktrees.
 */
export function WorktreeSwitcher() {
  const worktrees = useAtomValue(worktreesAtom);
  const repoInfo = useAtomValue(repositoryInfo);
  const currentRepoRoot = repoInfo?.repoRoot;

  // Find the current worktree based on repoRoot
  const currentWorktree = worktrees.find(wt => wt.path === currentRepoRoot);

  // Don't show anything if there are no worktrees or only the main worktree
  if (worktrees.length < 2) {
    return null;
  }

  const displayName = currentWorktree?.name ?? getLastPathSegment(currentRepoRoot ?? '');

  return (
    <div {...stylex.props(styles.container)}>
      <Tooltip
        trigger="click"
        component={dismiss => (
          <WorktreeDropdown
            worktrees={worktrees}
            currentPath={currentRepoRoot}
            dismiss={dismiss}
          />
        )}
        group="topbar"
        placement="bottom"
        title={t('Switch between worktrees')}>
        <Button icon data-testid="worktree-switcher-button">
          <Icon icon="folder-opened" />
          <span {...stylex.props(styles.buttonLabel)}>{displayName}</span>
          <Icon icon="chevron-down" />
        </Button>
      </Tooltip>
    </div>
  );
}

function WorktreeDropdown({
  worktrees,
  currentPath,
  dismiss,
}: {
  worktrees: WorktreeInfo[];
  currentPath: string | undefined;
  dismiss: () => void;
}) {
  const runOperation = useRunOperation();

  const handleWorktreeClick = (worktree: WorktreeInfo) => {
    if (worktree.path === currentPath) {
      dismiss();
      return;
    }
    // Change the cwd to the worktree path
    serverAPI.postMessage({
      type: 'changeCwd',
      cwd: worktree.path,
    });
    serverAPI.cwdChanged();
    dismiss();
  };

  const handleDeleteWorktree = (e: React.MouseEvent, worktree: WorktreeInfo) => {
    e.stopPropagation();
    runOperation(new WorktreeRemoveOperation(worktree.path));
  };

  // Sort worktrees: main first, then by name
  const sortedWorktrees = [...worktrees].sort((a, b) => {
    if (a.isMain && !b.isMain) {
      return -1;
    }
    if (!a.isMain && b.isMain) {
      return 1;
    }
    const aName = a.name ?? getLastPathSegment(a.path);
    const bName = b.name ?? getLastPathSegment(b.path);
    return aName.localeCompare(bName);
  });

  return (
    <Column xstyle={styles.dropdownContainer}>
      <div {...stylex.props(styles.header)}>
        <span {...stylex.props(styles.headerTitle)}>
          <T>Worktrees</T>
        </span>
        <span style={{fontSize: '11px', color: 'var(--foreground-sub)'}}>
          {worktrees.length} {worktrees.length === 1 ? 'worktree' : 'worktrees'}
        </span>
      </div>
      <ScrollY maxSize={300}>
        <div {...stylex.props(styles.worktreeList)}>
          {sortedWorktrees.length === 0 ? (
            <div {...stylex.props(styles.emptyState)}>
              <T>No worktrees found</T>
            </div>
          ) : (
            sortedWorktrees.map(worktree => {
              const isCurrent = worktree.path === currentPath;
              return (
                <div
                  key={worktree.path}
                  {...stylex.props(
                    styles.worktreeItem,
                    isCurrent && styles.worktreeItemCurrent,
                  )}
                  onClick={() => handleWorktreeClick(worktree)}
                  title={worktree.path}>
                  <Icon
                    icon={worktree.isMain ? 'home' : 'folder-opened'}
                    {...stylex.props(
                      styles.worktreeIcon,
                      worktree.isMain && styles.worktreeIconMain,
                    )}
                  />
                  <div {...stylex.props(styles.worktreeInfo)}>
                    <span {...stylex.props(styles.worktreeName)}>
                      {worktree.name ?? getLastPathSegment(worktree.path)}
                    </span>
                    <span {...stylex.props(styles.worktreePath)}>{worktree.path}</span>
                  </div>
                  {worktree.isMain && (
                    <span {...stylex.props(styles.mainBadge)}>main</span>
                  )}
                  {isCurrent && (
                    <span {...stylex.props(styles.currentBadge)}>
                      <T>current</T>
                    </span>
                  )}
                  {!worktree.isMain && !isCurrent && (
                    <Tooltip title={t('Delete this worktree')}>
                      <span
                        {...stylex.props(styles.deleteButton)}
                        onClick={e => handleDeleteWorktree(e, worktree)}>
                        <Icon icon="trash" />
                      </span>
                    </Tooltip>
                  )}
                </div>
              );
            })
          )}
        </div>
      </ScrollY>
    </Column>
  );
}

/**
 * Get the last segment of a path (folder name).
 */
function getLastPathSegment(path: string): string {
  const segments = path.split(/[/\\]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}
