/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Place, UIChangedFile, VisualChangedFileType} from './UncommittedChanges';
import type {UseUncommittedSelection} from './partialSelection';
import type {ChangedFileType, GeneratedStatus} from './types';
import type {ReactNode} from 'react';
import type {Comparison} from 'shared/Comparison';

import {type ChangedFilesDisplayType} from './ChangedFileDisplayTypePicker';
import {generatedStatusToLabel, generatedStatusDescription} from './GeneratedFile';
import {PartialFileSelectionWithMode} from './PartialFileSelection';
import {SuspenseBoundary} from './SuspenseBoundary';
import {Tooltip} from './Tooltip';
import {holdingAltAtom, holdingCtrlAtom} from './atoms/keyboardAtoms';
import {T, t} from './i18n';
import {AddOperation} from './operations/AddOperation';
import {ForgetOperation} from './operations/ForgetOperation';
import {PurgeOperation} from './operations/PurgeOperation';
import {ResolveOperation, ResolveTool} from './operations/ResolveOperation';
import {RevertOperation} from './operations/RevertOperation';
import {useRunOperation} from './operationsState';
import {useUncommittedSelection} from './partialSelection';
import platform from './platform';
import {optimisticMergeConflicts} from './previews';
import {useShowToast} from './toast';
import {succeedableRevset} from './types';
import {usePromise} from './usePromise';
import {VSCodeButton, VSCodeCheckbox} from '@vscode/webview-ui-toolkit/react';
import {useAtomValue} from 'jotai';
import React from 'react';
import {labelForComparison, revsetForComparison, ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {isMac} from 'shared/OperatingSystem';
import {basename, notEmpty} from 'shared/utils';

/**
 * Is the alt key currently held down, used to show full file paths.
 * On windows, this actually uses the ctrl key instead to avoid conflicting with OS focus behaviors.
 */
const holdingModifiedKeyAtom = isMac ? holdingAltAtom : holdingCtrlAtom;

export function File({
  file,
  displayType,
  comparison,
  selection,
  place,
  generatedStatus,
}: {
  file: UIChangedFile;
  displayType: ChangedFilesDisplayType;
  comparison: Comparison;
  selection?: UseUncommittedSelection;
  place?: Place;
  generatedStatus?: GeneratedStatus;
}) {
  const toast = useShowToast();
  const clipboardCopy = (text: string) => toast.copyAndShowToast(text);

  // Renamed files are files which have a copy field, where that path was also removed.

  // Visually show renamed files as if they were modified, even though sl treats them as added.
  const [statusName, icon] = nameAndIconForFileStatus[file.visualStatus];

  const generated = generatedStatusToLabel(generatedStatus);

  const contextMenu = useContextMenu(() => {
    const options = [
      {label: t('Copy File Path'), onClick: () => clipboardCopy(file.path)},
      {label: t('Copy Filename'), onClick: () => clipboardCopy(basename(file.path))},
      {label: t('Open File'), onClick: () => platform.openFile(file.path)},
    ];
    if (platform.openContainingFolder != null) {
      options.push({
        label: t('Open Containing Folder'),
        onClick: () => platform.openContainingFolder?.(file.path),
      });
    }
    if (platform.openDiff != null) {
      options.push({
        label: t('Open Diff View ($comparison)', {
          replace: {$comparison: labelForComparison(comparison)},
        }),
        onClick: () => platform.openDiff?.(file.path, comparison),
      });
    }
    return options;
  });

  // Hold "alt" key to show full file paths instead of short form.
  // This is a quick way to see where a file comes from without
  // needing to go through the menu to change the rendering type.
  const isHoldingAlt = useAtomValue(holdingModifiedKeyAtom);

  const tooltip = [file.tooltip, generatedStatusDescription(generatedStatus)]
    .filter(notEmpty)
    .join('\n\n');

  return (
    <>
      <div
        className={`changed-file file-${statusName} file-${generated}`}
        data-testid={`changed-file-${file.path}`}
        onContextMenu={contextMenu}
        key={file.path}
        tabIndex={0}
        onKeyPress={e => {
          if (e.key === 'Enter') {
            platform.openFile(file.path);
          }
        }}>
        <FileSelectionCheckbox file={file} selection={selection} />
        <span
          className="changed-file-path"
          onClick={() => {
            platform.openFile(file.path);
          }}>
          <Icon icon={icon} />
          <Tooltip title={tooltip} delayMs={2_000} placement="right">
            <span
              className="changed-file-path-text"
              onCopy={e => {
                const selection = document.getSelection();
                if (selection) {
                  // we inserted LTR markers, remove them again on copy
                  e.clipboardData.setData(
                    'text/plain',
                    selection.toString().replace(/\u200E/g, ''),
                  );
                  e.preventDefault();
                }
              }}>
              {escapeForRTL(
                displayType === 'tree'
                  ? file.path.slice(file.path.lastIndexOf('/') + 1)
                  : // Holding alt takes precedence over fish/short styles, but not tree.
                  displayType === 'fullPaths' || isHoldingAlt
                  ? file.path
                  : displayType === 'fish'
                  ? file.path
                      .split('/')
                      .map((a, i, arr) => (i === arr.length - 1 ? a : a[0]))
                      .join('/')
                  : file.label,
              )}
            </span>
          </Tooltip>
        </span>
        <FileActions file={file} comparison={comparison} place={place} />
      </div>
      {place === 'main' && selection?.isExpanded(file.path) && (
        <MaybePartialSelection file={file} />
      )}
    </>
  );
}

const revertableStatues = new Set(['M', 'R', '!']);
const conflictStatuses = new Set<ChangedFileType>(['U', 'Resolved']);
function FileActions({
  comparison,
  file,
  place,
}: {
  comparison: Comparison;
  file: UIChangedFile;
  place?: Place;
}) {
  const runOperation = useRunOperation();
  const conflicts = useAtomValue(optimisticMergeConflicts);

  const actions: Array<React.ReactNode> = [];

  if (platform.openDiff != null && !conflictStatuses.has(file.status)) {
    actions.push(
      <Tooltip title={t('Open diff view')} key="open-diff-view" delayMs={1000}>
        <VSCodeButton
          className="file-show-on-hover"
          appearance="icon"
          data-testid="file-open-diff-button"
          onClick={() => {
            platform.openDiff?.(file.path, comparison);
          }}>
          <Icon icon="request-changes" />
        </VSCodeButton>
      </Tooltip>,
    );
  }

  if (
    (revertableStatues.has(file.status) && comparison.type !== ComparisonType.Committed) ||
    // special case: reverting does actually work for added files in the head commit
    (comparison.type === ComparisonType.HeadChanges && file.status === 'A')
  ) {
    actions.push(
      <Tooltip
        title={
          comparison.type === ComparisonType.UncommittedChanges
            ? t('Revert back to last commit')
            : t('Revert changes made by this commit')
        }
        key="revert"
        delayMs={1000}>
        <VSCodeButton
          className="file-show-on-hover"
          key={file.path}
          appearance="icon"
          data-testid="file-revert-button"
          onClick={() => {
            platform
              .confirm(
                comparison.type === ComparisonType.UncommittedChanges
                  ? t('Are you sure you want to revert $file?', {replace: {$file: file.path}})
                  : t(
                      'Are you sure you want to revert $file back to how it was just before the last commit? Uncommitted changes to this file will be lost.',
                      {replace: {$file: file.path}},
                    ),
              )
              .then(ok => {
                if (!ok) {
                  return;
                }
                runOperation(
                  new RevertOperation(
                    [file.path],
                    comparison.type === ComparisonType.UncommittedChanges
                      ? undefined
                      : succeedableRevset(revsetForComparison(comparison)),
                  ),
                );
              });
          }}>
          <Icon icon="discard" />
        </VSCodeButton>
      </Tooltip>,
    );
  }

  if (comparison.type === ComparisonType.UncommittedChanges) {
    if (file.status === 'A') {
      actions.push(
        <Tooltip
          title={t('Stop tracking this file, without removing from the filesystem')}
          key="forget"
          delayMs={1000}>
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            onClick={() => {
              runOperation(new ForgetOperation(file.path));
            }}>
            <Icon icon="circle-slash" />
          </VSCodeButton>
        </Tooltip>,
      );
    } else if (file.status === '?') {
      actions.push(
        <Tooltip title={t('Start tracking this file')} key="add" delayMs={1000}>
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            onClick={() => runOperation(new AddOperation(file.path))}>
            <Icon icon="add" />
          </VSCodeButton>
        </Tooltip>,
        <Tooltip title={t('Remove this file from the filesystem')} key="remove" delayMs={1000}>
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            data-testid="file-action-delete"
            onClick={async () => {
              const ok = await platform.confirm(
                t('Are you sure you want to delete $file?', {replace: {$file: file.path}}),
              );
              if (!ok) {
                return;
              }
              runOperation(new PurgeOperation([file.path]));
            }}>
            <Icon icon="trash" />
          </VSCodeButton>
        </Tooltip>,
      );
    } else if (file.status === 'Resolved') {
      actions.push(
        <Tooltip title={t('Mark as unresolved')} key="unresolve-mark">
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
        <Tooltip title={t('Mark as resolved')} key="resolve-mark">
          <VSCodeButton
            className="file-show-on-hover"
            data-testid="file-action-resolve"
            key={file.path}
            appearance="icon"
            onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.mark))}>
            <Icon icon="check" />
          </VSCodeButton>
        </Tooltip>,
        <Tooltip title={t('Take local version')} key="resolve-local">
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.local))}>
            <Icon icon="fold-up" />
          </VSCodeButton>
        </Tooltip>,
        <Tooltip title={t('Take incoming version')} key="resolve-other">
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.other))}>
            <Icon icon="fold-down" />
          </VSCodeButton>
        </Tooltip>,
        <Tooltip title={t('Combine both incoming and local')} key="resolve-both">
          <VSCodeButton
            className="file-show-on-hover"
            key={file.path}
            appearance="icon"
            onClick={() => runOperation(new ResolveOperation(file.path, ResolveTool.both))}>
            <Icon icon="fold" />
          </VSCodeButton>
        </Tooltip>,
      );
    }

    if (place === 'main' && conflicts == null) {
      actions.push(<PartialSelectionAction file={file} key="partial-selection" />);
    }
  }
  return (
    <div className="file-actions" data-testid="file-actions">
      {actions}
    </div>
  );
}

/**
 * We render file paths with CSS text-direction: rtl,
 * which allows the ellipsis overflow to appear on the left.
 * However, rtl can have weird effects, such as moving leading '.' to the end.
 * To fix this, it's enough to add a left-to-right marker at the start of the path
 */
function escapeForRTL(s: string): ReactNode {
  return '\u200E' + s + '\u200E';
}

function FileSelectionCheckbox({
  file,
  selection,
}: {
  file: UIChangedFile;
  selection?: UseUncommittedSelection;
}) {
  return selection == null ? null : (
    <VSCodeCheckbox
      checked={selection.isFullyOrPartiallySelected(file.path)}
      indeterminate={selection.isPartiallySelected(file.path)}
      data-testid={'file-selection-checkbox'}
      // Note: Using `onClick` instead of `onChange` since onChange apparently fires when the controlled `checked` value changes,
      // which means this fires when using "select all" / "deselect all"
      onClick={e => {
        const checked = (e.target as HTMLInputElement).checked;
        if (checked) {
          if (file.renamedFrom != null) {
            // Selecting a renamed file also selects the original, so they are committed/amended together
            // the UI merges them visually anyway.
            selection.select(file.renamedFrom, file.path);
          } else {
            selection.select(file.path);
          }
        } else {
          if (file.renamedFrom != null) {
            selection.deselect(file.renamedFrom, file.path);
          } else {
            selection.deselect(file.path);
          }
        }
      }}
    />
  );
}

function PartialSelectionAction({file}: {file: UIChangedFile}) {
  const selection = useUncommittedSelection();

  const handleClick = () => {
    selection.toggleExpand(file.path);
  };

  return (
    <Tooltip
      component={() => (
        <div style={{maxWidth: '300px'}}>
          <div>
            <T
              replace={{
                $beta: (
                  <span
                    style={{
                      color: 'var(--scm-removed-foreground)',
                      marginLeft: 'var(--halfpad)',
                      fontSize: '80%',
                    }}>
                    (Beta)
                  </span>
                ),
              }}>
              Toggle chunk selection $beta
            </T>
          </div>
          <div>
            <T>
              Shows changed files in your commit and lets you select individual chunks or lines to
              include.
            </T>
          </div>
        </div>
      )}>
      <VSCodeButton className="file-show-on-hover" appearance="icon" onClick={handleClick}>
        <Icon icon="diff" />
      </VSCodeButton>
    </Tooltip>
  );
}

// Left margin to "indendent" by roughly a checkbox width.
const leftMarginStyle: React.CSSProperties = {marginLeft: 'calc(2.5 * var(--pad))'};

function MaybePartialSelection({file}: {file: UIChangedFile}) {
  const fallback = (
    <div style={leftMarginStyle}>
      <Icon icon="loading" />
    </div>
  );
  return (
    <SuspenseBoundary fallback={fallback}>
      <PartialSelectionPanel file={file} />
    </SuspenseBoundary>
  );
}

function PartialSelectionPanel({file}: {file: UIChangedFile}) {
  const path = file.path;
  const selection = useUncommittedSelection();
  const chunkSelect = usePromise(selection.getChunkSelect(path));

  return (
    <div style={leftMarginStyle}>
      <PartialFileSelectionWithMode
        chunkSelection={chunkSelect}
        setChunkSelection={state => selection.editChunkSelect(path, state)}
        mode="unified"
      />
    </div>
  );
}

/**
 * Map for changed files statuses into classNames (for color & styles) and icon names.
 */
const nameAndIconForFileStatus: Record<VisualChangedFileType, [string, string]> = {
  A: ['added', 'diff-added'],
  M: ['modified', 'diff-modified'],
  R: ['removed', 'diff-removed'],
  '?': ['ignored', 'question'],
  '!': ['missing', 'warning'],
  U: ['unresolved', 'diff-ignored'],
  Resolved: ['resolved', 'pass'],
  Renamed: ['modified', 'diff-renamed'],
  Copied: ['added', 'diff-added'],
};
