/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {WorktreeEntry} from './types';

import {Badge} from 'isl-components/Badge';
import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {Icon} from 'isl-components/Icon';
import {RadioGroup} from 'isl-components/Radio';
import {Subtle} from 'isl-components/Subtle';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtomValue} from 'jotai';
import {useCallback, useState} from 'react';
import {basename, dirname, guessPathSep, pathsAreIdentical} from 'shared/utils';
import serverAPI from './ClientToServerAPI';
import {Column, Row} from './ComponentUtils';
import css from './CwdSelector.module.css';
import {DropdownField, DropdownFields} from './DropdownFields';
import {Internal} from './Internal';
import {useFeatureFlagSync} from './featureFlags';
import {T, t} from './i18n';
import {AddWorktreeOperation} from './operations/AddWorktreeOperation';
import {RemoveWorktreeOperation} from './operations/RemoveWorktreeOperation';
import {RenameWorktreeOperation} from './operations/RenameWorktreeOperation';
import {useRunOperation} from './operationsState';
import platform from './platform';
import {applicationinfo, repositoryInfo, worktreeInfoData} from './serverAPIState';
import {useModal} from './useModal';

function useWorktreesEnabled(): boolean {
  const worktreesEnabled = useFeatureFlagSync(Internal.featureFlags?.Worktrees);
  const info = useAtomValue(repositoryInfo);

  // Only show worktrees for EdenFS repos that are not git-based
  return worktreesEnabled && info?.isEdenFs === true && info?.codeReviewSystem.type !== 'github';
}

/** Top-bar button, next to the branches button, showing the same worktree info as the repo dropdown. */
export function WorktreeButton() {
  const enabled = useWorktreesEnabled();
  if (!enabled) {
    return null;
  }
  return (
    <Tooltip
      title={<T>Worktrees</T>}
      trigger="click"
      placement="bottom"
      group="topbar"
      component={dismiss => (
        <DropdownFields
          title={<T>Worktrees</T>}
          icon="worktree"
          data-testid="worktree-details-dropdown">
          <WorktreeSection dismiss={dismiss} />
        </DropdownFields>
      )}>
      <Button icon data-testid="worktree-button">
        <Icon icon="worktree" />
      </Button>
    </Tooltip>
  );
}

export function WorktreeSection({dismiss}: {dismiss: () => unknown}) {
  const enabled = useWorktreesEnabled();
  if (!enabled) {
    return null;
  }
  return (
    <DropdownField
      title={
        <Tooltip
          title={t(
            'Worktrees are lightweight copies of your repository, like branches with their own working copy. Useful for working in parallel on the same machine.',
          )}>
          <Row>
            <T>Available Worktrees</T> <Icon icon="question" />
          </Row>
        </Tooltip>
      }>
      <WorktreeDropdown dismiss={dismiss} />
    </DropdownField>
  );
}

function WorktreeDropdown({dismiss}: {dismiss: () => unknown}) {
  const info = useAtomValue(repositoryInfo);
  const worktreeInfo = useAtomValue(worktreeInfoData);
  const repoRoot = info?.repoRoot ?? '';
  const runOperation = useRunOperation();
  const showModal = useModal();

  const allWorktrees = worktreeInfo?.worktrees ?? [];
  const sortedWorktrees = [...allWorktrees].sort((a, b) => {
    if (a.role === 'main') {
      return -1;
    }
    if (b.role === 'main') {
      return 1;
    }
    return basename(a.path, guessPathSep(a.path)).localeCompare(
      basename(b.path, guessPathSep(b.path)),
    );
  });

  const renderWorktreeRow = (wt: WorktreeEntry) => {
    const isCurrent = pathsAreIdentical(wt.path, repoRoot);
    const wtBasename = basename(wt.path, guessPathSep(wt.path));
    const rowClass = isCurrent ? css.worktreeRowCurrent : css.worktreeRow;
    const hasLabel = wt.label != null && wt.label !== '';
    return (
      <WorktreeRowWithHover
        key={wt.path}
        rowClass={rowClass}
        isCurrent={isCurrent}
        wt={wt}
        wtBasename={wtBasename}
        hasLabel={hasLabel}
        runOperation={runOperation}
        showModal={showModal}
        dismiss={dismiss}
      />
    );
  };

  return (
    <div className={css.worktreeSection} data-testid="worktree-section">
      {sortedWorktrees.map(wt => renderWorktreeRow(wt))}
      <AddWorktreeButton
        dismiss={dismiss}
        repoRoot={sortedWorktrees.find(wt => wt.role === 'main')?.path ?? repoRoot}
        existingWorktreePaths={allWorktrees.map(wt => wt.path)}
      />
    </div>
  );
}

function WorktreeRowWithHover({
  rowClass,
  isCurrent,
  wt,
  wtBasename,
  hasLabel,
  runOperation,
  showModal,
  dismiss,
}: {
  rowClass: string;
  isCurrent: boolean | undefined;
  wt: WorktreeEntry;
  wtBasename: string;
  hasLabel: boolean;
  runOperation: ReturnType<typeof useRunOperation>;
  showModal: ReturnType<typeof useModal>;
  dismiss: () => unknown;
}) {
  const appInfo = useAtomValue(applicationinfo);
  return (
    <div
      key={wt.path}
      className={rowClass}
      data-testid={isCurrent ? 'current-worktree' : 'sibling-worktree'}>
      <code className={css.worktreePath} title={wt.path}>
        {hasLabel ? wt.label : wtBasename}
      </code>
      {isCurrent ? (
        <Badge className={css.activeBadge}>Active</Badge>
      ) : (
        <div className={css.worktreeActions}>
          <Tooltip title={t('Rename this worktree')}>
            <Button
              icon
              data-testid="worktree-rename-button"
              onClick={async () => {
                dismiss();
                const result = await showModal<string | undefined>({
                  type: 'custom',
                  title: <T>Rename Worktree</T>,
                  icon: 'worktree',
                  component: ({returnResultAndDismiss}) => (
                    <RenameWorktreeModal
                      returnResultAndDismiss={returnResultAndDismiss}
                      currentLabel={wt.label ?? ''}
                      wtBasename={wtBasename}
                    />
                  ),
                });
                if (result !== undefined) {
                  await runOperation(
                    new RenameWorktreeOperation(wt.path, result || undefined),
                    true,
                  );
                }
              }}>
              <Icon icon="edit" />
            </Button>
          </Tooltip>
          <Tooltip title={t('Switch to this worktree')}>
            <Button
              icon
              data-testid="worktree-switch-button"
              onClick={async () => {
                dismiss();
                if (platform.platformName !== 'vscode') {
                  changeCwd(wt.path);
                  return;
                }
                const isBasecamp = appInfo?.isBasecamp === true;
                const newWindowLabel = isBasecamp ? t('Open in New Tile') : t('Open in New Window');
                const choice = await showModal({
                  type: 'confirm',
                  title: <T>Switch Worktree</T>,
                  icon: 'worktree',
                  message: (
                    <Column alignStart style={{gap: 'var(--pad)'}}>
                      <Row>
                        <T replace={{$path: <code>{wtBasename}</code>}}>
                          Switch to worktree $path?
                        </T>
                      </Row>
                      {!isBasecamp && (
                        <Row>
                          <Subtle>
                            <T>Opening in current window will reload the editor.</T>
                          </Subtle>
                        </Row>
                      )}
                    </Column>
                  ),
                  buttons: isBasecamp
                    ? [{label: newWindowLabel, primary: true}]
                    : [
                        {label: t('Open in Current Window')},
                        {label: newWindowLabel, primary: true},
                      ],
                });
                if (choice != null) {
                  openWorktreeInWindow(wt.path, choice.label === newWindowLabel);
                }
              }}>
              <Icon icon="arrow-swap" />
            </Button>
          </Tooltip>
          {wt.role === 'main' ? (
            <Tooltip title={t('The main worktree cannot be removed')}>
              <Button icon disabled data-testid="worktree-remove-button">
                <Icon icon="trash" />
              </Button>
            </Tooltip>
          ) : (
            <Tooltip title={t('Remove this worktree at $path', {replace: {$path: wt.path}})}>
              <Button
                icon
                data-testid="worktree-remove-button"
                onClick={async () => {
                  dismiss();
                  const confirmed = await showModal({
                    type: 'confirm',
                    title: <T>Remove Worktree</T>,
                    icon: 'worktree',
                    message: (
                      <span>
                        <Row>
                          <T replace={{$path: <code>{hasLabel ? wt.label : wtBasename}</code>}}>
                            Are you sure you want to remove the worktree $path?
                          </T>
                        </Row>
                        <Row style={{marginTop: 'var(--pad)'}}>
                          <Subtle>
                            <T>
                              Any uncommitted changes and shelves in this worktree will be lost
                              forever.
                            </T>
                          </Subtle>
                        </Row>
                      </span>
                    ),
                    buttons: [{label: t('Cancel')}, {label: t('Remove'), primary: true}],
                  });
                  if (confirmed?.label === t('Remove')) {
                    await runOperation(new RemoveWorktreeOperation(wt.path), true);
                  }
                }}>
                <Icon icon="trash" />
              </Button>
            </Tooltip>
          )}
        </div>
      )}
    </div>
  );
}

function AddWorktreeButton({
  dismiss,
  repoRoot,
  existingWorktreePaths,
}: {
  dismiss: () => unknown;
  repoRoot: string;
  existingWorktreePaths: string[];
}) {
  const showModal = useModal();
  const runOperation = useRunOperation();
  const sep = guessPathSep(repoRoot);
  const parentDir = dirname(repoRoot, sep);
  const repoName = basename(repoRoot, sep);
  const existingBasenames = new Set(existingWorktreePaths.map(p => basename(p, guessPathSep(p))));
  let suffix = 2;
  while (existingBasenames.has(`${repoName}_${suffix}`)) {
    suffix++;
  }
  const defaultDest = `${parentDir}${sep}${repoName}.worktrees${sep}${repoName}_${suffix}`;

  const onClickAdd = useCallback(async () => {
    dismiss();
    const result = await showModal<AddWorktreeResult>({
      type: 'custom',
      title: (
        <Row>
          <T>Add Worktree</T>{' '}
          <Tooltip
            title={t(
              'Worktrees are lightweight copies of your repository, like branches with their own working copy. Useful for working in parallel on the same machine.',
            )}>
            <Icon icon="question" />
          </Tooltip>
        </Row>
      ),
      icon: 'worktree',
      maxWidth: 500,
      component: ({returnResultAndDismiss}) => (
        <AddWorktreeModal
          returnResultAndDismiss={returnResultAndDismiss}
          defaultDest={defaultDest}
        />
      ),
    });
    if (result != null) {
      // Only show the creating dialog if we're going to open the worktree after creation
      const showSpinner = result.openIn !== 'none';
      let spinnerDismiss: (() => void) | undefined;
      if (showSpinner) {
        spinnerDismiss = await new Promise<() => void>(resolve => {
          showModal({
            type: 'custom',
            title: <T>Creating Worktree</T>,
            icon: 'worktree',
            component: ({returnResultAndDismiss}) => {
              resolve(() => returnResultAndDismiss(undefined));
              return (
                <div className={css.worktreeSpinner}>
                  <Icon icon="loading" />
                  <T>Creating worktree at</T> <code>{result.destPath}</code>
                </div>
              );
            },
          });
        });
      }
      try {
        await runOperation(
          new AddWorktreeOperation(result.destPath, result.label || undefined),
          true,
        );
      } finally {
        spinnerDismiss?.();
      }
      if (result.openIn === 'current') {
        changeCwd(result.destPath);
      } else if (result.openIn === 'new') {
        serverAPI.postMessage({type: 'platform/openInNewWindow', path: result.destPath});
      }
    }
  }, [dismiss, showModal, runOperation, defaultDest]);

  return (
    <Button
      data-testid="add-worktree-button"
      className={css.addWorktreeButton}
      onClick={onClickAdd}>
      <Icon icon="plus" /> <T>Add Worktree</T>
    </Button>
  );
}

type AddWorktreeResult = {
  destPath: string;
  label: string;
  openIn: 'current' | 'new' | 'none';
};

function AddWorktreeModal({
  returnResultAndDismiss,
  defaultDest,
}: {
  returnResultAndDismiss: (result: AddWorktreeResult) => void;
  defaultDest: string;
}) {
  const [destPath, setDestPath] = useState(defaultDest);
  const [label, setLabel] = useState('');
  const appInfo = useAtomValue(applicationinfo);
  const isVSCode = platform.platformName === 'vscode';
  const [openIn, setOpenIn] = useState<AddWorktreeResult['openIn']>(isVSCode ? 'new' : 'none');
  const [activate, setActivate] = useState(false);
  const [isEditingPath, setIsEditingPath] = useState(false);

  return (
    <div className={css.addWorktreeForm} data-testid="add-worktree-form">
      <TextField
        data-testid="add-worktree-label"
        placeholder={t('Label (optional)')}
        value={label}
        onInput={e => setLabel(e.currentTarget?.value ?? '')}
      />
      <div className={css.worktreeLocationSection}>
        <Subtle className={css.worktreeLocationLabel}>
          <Tooltip title={<T>Location on disk where the worktree files will be created</T>}>
            <T>Worktree root path</T>
          </Tooltip>
        </Subtle>
        {isEditingPath ? (
          <TextField
            data-testid="add-worktree-path"
            value={destPath}
            onInput={e => setDestPath(e.currentTarget?.value ?? '')}
            onBlur={() => setIsEditingPath(false)}
            autoFocus
          />
        ) : (
          <div
            className={css.worktreePathDisplay}
            onClick={() => setIsEditingPath(true)}
            data-testid="add-worktree-edit-path">
            <code className={css.worktreePathText}>{destPath}</code>
            <Icon icon="edit" className={css.worktreePathEditIcon} />
          </div>
        )}
      </div>
      {isVSCode ? (
        <RadioGroup
          choices={
            [
              {
                title: (
                  <Row>
                    {appInfo?.isBasecamp ? <T>Open in new tile</T> : <T>Open in new window</T>}
                    <Badge>Recommended</Badge>
                  </Row>
                ),
                value: 'new',
              },
              ...(appInfo?.isBasecamp
                ? []
                : [{title: <T>Open in current window</T>, value: 'current'}]),
              {title: <T>Don't open</T>, value: 'none'},
            ] as Array<{value: AddWorktreeResult['openIn']; title: ReactNode}>
          }
          current={openIn}
          onChange={setOpenIn}
        />
      ) : (
        <Checkbox
          checked={activate}
          onChange={checked => {
            setActivate(checked);
            setOpenIn(checked ? 'current' : 'none');
          }}>
          <T>Activate</T>
        </Checkbox>
      )}
      <div className={css.addWorktreeFormActions}>
        <Button
          primary
          data-testid="add-worktree-submit"
          disabled={destPath.trim() === ''}
          onClick={() =>
            returnResultAndDismiss({
              destPath: destPath.trim(),
              label: label.trim(),
              openIn,
            })
          }>
          <T>Create</T>
        </Button>
      </div>
    </div>
  );
}

export function RenameWorktreeModal({
  returnResultAndDismiss,
  currentLabel,
  wtBasename,
}: {
  returnResultAndDismiss: (result: string | undefined) => void;
  currentLabel: string;
  wtBasename: string;
}) {
  const [newLabel, setNewLabel] = useState(currentLabel);

  return (
    <div className={css.addWorktreeForm} data-testid="rename-worktree-form">
      <Subtle>
        <T replace={{$path: <code>{wtBasename}</code>}}>Set a display label for worktree $path</T>
      </Subtle>
      <TextField
        data-testid="rename-worktree-label"
        placeholder={t('Label (leave empty to remove)')}
        value={newLabel}
        onInput={e => setNewLabel(e.currentTarget?.value ?? '')}
        autoFocus
      />
      <div className={css.addWorktreeFormActions}>
        <Button
          primary
          data-testid="rename-worktree-submit"
          onClick={() => returnResultAndDismiss(newLabel.trim())}>
          {newLabel.trim() === '' ? t('Remove label') : t('Save')}
        </Button>
      </div>
    </div>
  );
}

export function changeCwd(newCwd: string) {
  serverAPI.postMessage({
    type: 'changeCwd',
    cwd: newCwd,
  });
  serverAPI.cwdChanged();
}

/** Ask the host platform to open `path` either in the current window or a new one. */
export function openWorktreeInWindow(path: string, newWindow: boolean) {
  if (newWindow) {
    serverAPI.postMessage({type: 'platform/openInNewWindow', path});
  } else {
    serverAPI.postMessage({type: 'platform/openFolder', path});
  }
}
