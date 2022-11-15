/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChangedFile, ChangedFileType, RepoRelativePath} from './types';
import type {SetterOrUpdater} from 'recoil';
import type {EnsureAssignedTogether} from 'shared/EnsureAssignedTogether';

import {islDrawerState} from './App';
import {commitFieldsBeingEdited, commitMode} from './CommitInfo';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {ErrorNotice} from './ErrorNotice';
import {Icon} from './Icon';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {AbortMergeOperation} from './operations/AbortMergeOperation';
import {AddOperation} from './operations/AddOperation';
import {AmendOperation} from './operations/AmendOperation';
import {CommitOperation} from './operations/CommitOperation';
import {ContinueOperation} from './operations/ContinueMergeOperation';
import {DiscardOperation} from './operations/DiscardOperation';
import {PurgeOperation} from './operations/PurgeOperation';
import {ResolveOperation, ResolveTool} from './operations/ResolveOperation';
import {RevertOperation} from './operations/RevertOperation';
import platform from './platform';
import {uncommittedChangesWithPreviews} from './previews';
import {selectedCommits} from './selection';
import {
  latestHeadCommit,
  mergeConflicts,
  uncommittedChangesFetchError,
  useRunOperation,
} from './serverAPIState';
import {VSCodeButton, VSCodeCheckbox, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useEffect, useRef} from 'react';
import {atom, useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {ComparisonType} from 'shared/Comparison';

import './UncommittedChanges.css';

export function ChangedFiles({
  files,
  deselectedFiles,
  setDeselectedFiles,
}: {files: Array<ChangedFile>} & EnsureAssignedTogether<{
  deselectedFiles?: Set<string>;
  setDeselectedFiles?: (newDeselected: Set<string>) => unknown;
}>) {
  return (
    <div className="changed-files">
      {files.map(file => {
        const [statusName, icon] = nameAndIconForFileStatus[file.status];
        return (
          <div
            className={`changed-file file-${statusName}`}
            key={file.path}
            tabIndex={0}
            onKeyPress={e => {
              if (e.key === 'Enter') {
                platform.openFile(file.path);
              }
            }}>
            {deselectedFiles == null ? null : (
              <VSCodeCheckbox
                checked={!deselectedFiles.has(file.path)}
                // Note: Using `onClick` instead of `onChange` since onChange apparently fires when the controlled `checked` value changes,
                // which means this fires when using "select all" / "deselect all"
                onClick={e => {
                  const newDeselected = new Set(deselectedFiles);
                  const checked = (e.target as HTMLInputElement).checked;
                  if (checked) {
                    if (newDeselected.has(file.path)) {
                      newDeselected.delete(file.path);
                      setDeselectedFiles?.(newDeselected);
                    }
                  } else {
                    if (!newDeselected.has(file.path)) {
                      newDeselected.add(file.path);
                      setDeselectedFiles?.(newDeselected);
                    }
                  }
                }}
              />
            )}
            <Icon icon={icon} />
            <span
              className="changed-file-path"
              title={file.path}
              onClick={() => {
                platform.openFile(file.path);
              }}>
              {file.path}
            </span>
            <FileActions file={file} />
          </div>
        );
      })}
    </div>
  );
}

export function UncommittedChanges({place}: {place: 'main' | 'amend sidebar' | 'commit sidebar'}) {
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const error = useRecoilValue(uncommittedChangesFetchError);
  // TODO: use treeWithPreviews instead, and update CommitOperation
  const headCommit = useRecoilValue(latestHeadCommit);

  const conflicts = useRecoilValue(mergeConflicts);

  const [deselectedFiles, setDeselectedFiles] = useDeselectedFiles(uncommittedChanges);
  const commitTitleRef = useRef(null);

  const runOperation = useRunOperation();

  const openCommitForm = useRecoilCallback(({set, reset}) => (which: 'commit' | 'amend') => {
    // make sure view is expanded
    set(islDrawerState, val => ({...val, right: {...val.right, collapsed: false}}));

    // show head commit & set to correct mode
    reset(selectedCommits);
    set(commitMode, which);

    // Start editing fields when amending so you can go right into typing.
    if (which === 'amend') {
      set(commitFieldsBeingEdited, {
        title: true,
        description: true,
        // we have to explicitly keep this change to fieldsBeingEdited because otherwise it would be reset by effects.
        forceWhileOnHead: true,
      });
    }
  });

  if (error) {
    return <ErrorNotice title={t('Failed to fetch Uncommitted Changes')} error={error} />;
  }
  if (uncommittedChanges.length === 0) {
    return null;
  }
  const allFilesSelected = deselectedFiles.size === 0;
  const noFilesSelected = deselectedFiles.size === uncommittedChanges.length;

  const allConflictsResolved =
    conflicts?.files?.every(conflict => conflict.status === 'Resolved') ?? false;
  return (
    <div className="uncommitted-changes">
      {conflicts != null ? (
        <div className="conflicts-header">
          <strong>
            {allConflictsResolved ? (
              <T>All Merge Conflicts Resolved</T>
            ) : (
              <T>Unresolved Merge Conflicts</T>
            )}
          </strong>
          {conflicts.state === 'loading' ? (
            <div data-testid="merge-conflicts-spinner">
              <Icon icon="loading" />
            </div>
          ) : null}
          {allConflictsResolved ? null : (
            <T replace={{$cmd: conflicts.command}}>Resolve conflicts to continue $cmd</T>
          )}
        </div>
      ) : null}
      <div className="button-row">
        {conflicts != null ? (
          <>
            <VSCodeButton
              appearance={allConflictsResolved ? 'primary' : 'icon'}
              key="continue"
              disabled={!allConflictsResolved}
              data-testid="conflict-continue-button"
              onClick={() => {
                runOperation(new ContinueOperation());
              }}>
              <Icon slot="start" icon="debug-continue" />
              <T>Continue</T>
            </VSCodeButton>
            <VSCodeButton
              appearance="icon"
              key="abort"
              onClick={() => {
                runOperation(new AbortMergeOperation(conflicts));
              }}>
              <Icon slot="start" icon="circle-slash" />
              <T>Abort</T>
            </VSCodeButton>
          </>
        ) : (
          <>
            <OpenComparisonViewButton
              comparison={{
                type:
                  place === 'amend sidebar'
                    ? ComparisonType.HeadChanges
                    : ComparisonType.UncommittedChanges,
              }}
            />
            <VSCodeButton
              appearance="icon"
              key="select-all"
              disabled={allFilesSelected}
              onClick={() => {
                setDeselectedFiles(new Set());
              }}>
              <Icon slot="start" icon="check-all" />
              <T>Select All</T>
            </VSCodeButton>
            <VSCodeButton
              appearance="icon"
              key="deselect-all"
              disabled={noFilesSelected}
              onClick={() => {
                setDeselectedFiles(new Set(uncommittedChanges.map(file => file.path)));
              }}>
              <Icon slot="start" icon="close-all" />
              <T>Deselect All</T>
            </VSCodeButton>
            <Tooltip
              delayMs={DOCUMENTATION_DELAY}
              title={t('discardTooltip', {
                count: uncommittedChanges.length - deselectedFiles.size,
              })}>
              <VSCodeButton
                appearance="icon"
                disabled={noFilesSelected}
                onClick={() => {
                  const selectedFiles = uncommittedChanges
                    .filter(file => !deselectedFiles.has(file.path))
                    .map(file => file.path);
                  platform
                    .confirm(t('confirmDiscardChanges', {count: selectedFiles.length}))
                    .then(ok => {
                      if (!ok) {
                        return;
                      }
                      if (deselectedFiles.size === 0) {
                        // all changes selected -> use clean goto rather than reverting each file. This is generally faster.

                        // to "discard", we need to both remove uncommitted changes
                        runOperation(new DiscardOperation());
                        // ...and delete untracked files.
                        // Technically we only need to do the purge when we have untracked files, though there's a chance there's files we don't know about yet while status is running.
                        runOperation(new PurgeOperation());
                      } else {
                        // only a subset of files selected -> we need to revert selected files individually
                        runOperation(new RevertOperation(selectedFiles));
                      }
                    });
                }}>
                <Icon slot="start" icon="trashcan" />
                <T>Discard</T>
              </VSCodeButton>
            </Tooltip>
          </>
        )}
      </div>
      {conflicts?.files != null ? (
        <ChangedFiles files={conflicts.files} />
      ) : (
        <ChangedFiles
          files={uncommittedChanges}
          deselectedFiles={deselectedFiles}
          setDeselectedFiles={setDeselectedFiles}
        />
      )}
      {conflicts != null || place !== 'main' ? null : (
        <div className="button-rows">
          <div className="button-row">
            <span className="quick-commit-inputs">
              <VSCodeButton
                appearance="icon"
                disabled={noFilesSelected}
                onClick={() => {
                  const title =
                    (commitTitleRef.current as HTMLInputElement | null)?.value ||
                    t('Temporary Commit');
                  const filesToCommit =
                    deselectedFiles.size === 0
                      ? // all files
                        undefined
                      : // only files not unchecked
                        uncommittedChanges
                          .filter(file => !deselectedFiles.has(file.path))
                          .map(file => file.path);
                  runOperation(
                    new CommitOperation(
                      {title, description: ''},
                      headCommit?.hash ?? '',
                      filesToCommit,
                    ),
                  );
                }}>
                <Icon slot="start" icon="plus" />
                <T>Commit</T>
              </VSCodeButton>
              <VSCodeTextField placeholder="Title" ref={commitTitleRef} />
            </span>
            <VSCodeButton
              appearance="icon"
              className="show-on-hover"
              onClick={() => {
                openCommitForm('commit');
              }}>
              <Icon slot="start" icon="edit" />
              <T>Commit as...</T>
            </VSCodeButton>
          </div>
          <div className="button-row">
            <VSCodeButton
              appearance="icon"
              disabled={noFilesSelected}
              data-testid="uncommitted-changes-quick-amend-button"
              onClick={() => {
                const filesToCommit =
                  deselectedFiles.size === 0
                    ? // all files
                      undefined
                    : // only files not unchecked
                      uncommittedChanges
                        .filter(file => !deselectedFiles.has(file.path))
                        .map(file => file.path);
                runOperation(new AmendOperation(filesToCommit));
              }}>
              <Icon slot="start" icon="debug-step-into" />
              <T>Amend</T>
            </VSCodeButton>
            <VSCodeButton
              appearance="icon"
              className="show-on-hover"
              onClick={() => {
                openCommitForm('amend');
              }}>
              <Icon slot="start" icon="edit" />
              <T>Amend as...</T>
            </VSCodeButton>
          </div>
        </div>
      )}
    </div>
  );
}

function FileActions({file}: {file: ChangedFile}) {
  const runOperation = useRunOperation();
  const actions: Array<React.ReactNode> = [];
  if (file.status === '?') {
    actions.push(
      <VSCodeButton
        key={file.path}
        appearance="icon"
        onClick={() => runOperation(new AddOperation(file.path))}>
        <Icon icon="add" />
      </VSCodeButton>,
    );
  } else if (file.status === 'Resolved') {
    actions.push(
      <Tooltip title="Mark as unresolved" key="unresolve-mark">
        <VSCodeButton
          key={file.path}
          appearance="icon"
          onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.unmark))}>
          <Icon icon="circle-slash" />
        </VSCodeButton>
      </Tooltip>,
    );
  } else if (file.status === 'U') {
    actions.push(
      <Tooltip title="Mark as resolved" key="resolve-mark">
        <VSCodeButton
          className="show-on-hover"
          key={file.path}
          appearance="icon"
          onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.mark))}>
          <Icon icon="check" />
        </VSCodeButton>
      </Tooltip>,
      <Tooltip title="Take local version" key="resolve-local">
        <VSCodeButton
          className="show-on-hover"
          key={file.path}
          appearance="icon"
          onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.local))}>
          <Icon icon="fold-up" />
        </VSCodeButton>
      </Tooltip>,
      <Tooltip title="Take incoming version" key="resolve-other">
        <VSCodeButton
          className="show-on-hover"
          key={file.path}
          appearance="icon"
          onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.other))}>
          <Icon icon="fold-down" />
        </VSCodeButton>
      </Tooltip>,
      <Tooltip title="Combine both incoming and local" key="resolve-both">
        <VSCodeButton
          className="show-on-hover"
          key={file.path}
          appearance="icon"
          onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.both))}>
          <Icon icon="fold" />
        </VSCodeButton>
      </Tooltip>,
    );
  }
  return <div className="file-actions">{actions}</div>;
}

/**
 * The subset of uncommitted changes which have been unchecked in the list.
 * Deselected files won't be committed or amended.
 */
export const deselectedUncommittedChanges = atom<Set<RepoRelativePath>>({
  key: 'deselectedUncommittedChanges',
  default: new Set(),
});

function useDeselectedFiles(
  files: Array<ChangedFile>,
): [Set<RepoRelativePath>, SetterOrUpdater<Set<RepoRelativePath>>] {
  const [deselectedFiles, setDeselectedFiles] = useRecoilState(deselectedUncommittedChanges);
  useEffect(() => {
    const allPaths = new Set(files.map(file => file.path));
    const updatedDeselected = new Set(deselectedFiles);
    let anythingChanged = false;
    for (const deselected of deselectedFiles) {
      if (!allPaths.has(deselected)) {
        // invariant: deselectedFiles is a subset of uncommittedChangesWithPreviews
        updatedDeselected.delete(deselected);
        anythingChanged = true;
      }
    }
    if (anythingChanged) {
      setDeselectedFiles(updatedDeselected);
    }
  }, [files, deselectedFiles, setDeselectedFiles]);
  return [deselectedFiles, setDeselectedFiles];
}

/**
 * Map for changed files statuses into classNames (for color & styles) and icon names.
 */
const nameAndIconForFileStatus: Record<ChangedFileType, [string, string]> = {
  A: ['added', 'diff-added'],
  M: ['modified', 'diff-modified'],
  R: ['removed', 'diff-removed'],
  '?': ['ignored', 'question'],
  '!': ['ignored', 'warning'],
  U: ['unresolved', 'diff-ignored'],
  Resolved: ['resolved', 'pass'],
};
