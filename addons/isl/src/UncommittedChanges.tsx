/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitMessageFields} from './CommitInfoView/types';
import type {UseUncommittedSelection} from './partialSelection';
import type {ChangedFile, ChangedFileType, MergeConflicts, RepoRelativePath} from './types';
import type {MutableRefObject} from 'react';
import type {Comparison} from 'shared/Comparison';

import {Banner, BannerKind} from './Banner';
import {File} from './ChangedFile';
import {
  ChangedFileDisplayTypePicker,
  type ChangedFilesDisplayType,
  changedFilesDisplayType,
} from './ChangedFileDisplayTypePicker';
import {Collapsable} from './Collapsable';
import {
  commitMessageTemplate,
  commitMode,
  editedCommitMessages,
  forceNextCommitToEditAllFields,
} from './CommitInfoView/CommitInfoState';
import {
  commitMessageFieldsSchema,
  commitMessageFieldsToString,
} from './CommitInfoView/CommitMessageFields';
import {temporaryCommitTitle} from './CommitTitle';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {ErrorNotice} from './ErrorNotice';
import {FileTree, FileTreeFolderHeader} from './FileTree';
import {useGeneratedFileStatuses} from './GeneratedFile';
import {Internal} from './Internal';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {latestCommitMessageFields} from './codeReview/CodeReviewInfo';
import {Badge} from './components/Badge';
import {islDrawerState} from './drawerState';
import {T, t} from './i18n';
import {localStorageBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {AbortMergeOperation} from './operations/AbortMergeOperation';
import {AddRemoveOperation} from './operations/AddRemoveOperation';
import {getAmendOperation} from './operations/AmendOperation';
import {getCommitOperation} from './operations/CommitOperation';
import {ContinueOperation} from './operations/ContinueMergeOperation';
import {DiscardOperation, PartialDiscardOperation} from './operations/DiscardOperation';
import {PurgeOperation} from './operations/PurgeOperation';
import {RevertOperation} from './operations/RevertOperation';
import {getShelveOperation} from './operations/ShelveOperation';
import {operationList, useRunOperation} from './operationsState';
import {useUncommittedSelection} from './partialSelection';
import platform from './platform';
import {
  optimisticMergeConflicts,
  uncommittedChangesWithPreviews,
  useIsOperationRunningOrQueued,
} from './previews';
import {selectedCommits} from './selection';
import {latestHeadCommit, uncommittedChangesFetchError} from './serverAPIState';
import {GeneratedStatus} from './types';
import {VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useAtom, useAtomValue} from 'jotai';
import React, {useCallback, useMemo, useEffect, useRef, useState} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';
import {useDeepMemo} from 'shared/hooks';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {group, notEmpty, partition} from 'shared/utils';

import './UncommittedChanges.css';

export type UIChangedFile = {
  path: RepoRelativePath;
  // disambiguated path, or rename with arrow
  label: string;
  status: ChangedFileType;
  visualStatus: VisualChangedFileType;
  copiedFrom?: RepoRelativePath;
  renamedFrom?: RepoRelativePath;
  tooltip: string;
};

function processCopiesAndRenames(files: Array<ChangedFile>): Array<UIChangedFile> {
  const disambiguousPaths = minimalDisambiguousPaths(files.map(file => file.path));
  const copySources = new Set(files.map(file => file.copy).filter(notEmpty));
  const removedFiles = new Set(files.filter(file => file.status === 'R').map(file => file.path));

  return (
    files
      .map((file, i) => {
        const minimalName = disambiguousPaths[i];
        let fileLabel = minimalName;
        let tooltip = file.path;
        let copiedFrom;
        let renamedFrom;
        let visualStatus: VisualChangedFileType = file.status;
        if (file.copy != null) {
          // Disambiguate between original file and the newly copy's name,
          // instead of disambiguating among all file names.
          const [originalName, copiedName] = minimalDisambiguousPaths([file.copy, file.path]);
          fileLabel = `${originalName} → ${copiedName}`;
          if (removedFiles.has(file.copy)) {
            renamedFrom = file.copy;
            tooltip = t('$newPath\n\nThis file was renamed from $originalPath', {
              replace: {$newPath: file.path, $originalPath: file.copy},
            });
            visualStatus = 'Renamed';
          } else {
            copiedFrom = file.copy;
            tooltip = t('$newPath\n\nThis file was copied from $originalPath', {
              replace: {$newPath: file.path, $originalPath: file.copy},
            });
            visualStatus = 'Copied';
          }
        }

        return {
          path: file.path,
          label: fileLabel,
          status: file.status,
          visualStatus,
          copiedFrom,
          renamedFrom,
          tooltip,
        };
      })
      // Hide files that were renamed. This comes after the map since we need to use the index to refer to minimalDisambiguousPaths
      .filter(file => !(file.status === 'R' && copySources.has(file.path)))
      .sort((a, b) =>
        a.visualStatus === b.visualStatus
          ? a.path.localeCompare(b.path)
          : sortKeyForStatus[a.visualStatus] - sortKeyForStatus[b.visualStatus],
      )
  );
}

export type VisualChangedFileType = ChangedFileType | 'Renamed' | 'Copied';

const sortKeyForStatus: Record<VisualChangedFileType, number> = {
  M: 0,
  Renamed: 1,
  A: 2,
  Copied: 3,
  R: 4,
  '!': 5,
  '?': 6,
  U: 7,
  Resolved: 8,
};

type SectionProps = Omit<React.ComponentProps<typeof LinearFileList>, 'files'> & {
  filesByPrefix: Map<string, Array<UIChangedFile>>;
};

function SectionedFileList({filesByPrefix, ...rest}: SectionProps) {
  const [collapsedSections, setCollapsedSections] = useState(new Set<string>());
  return (
    <div className="file-tree">
      {Array.from(filesByPrefix.entries(), ([prefix, files]) => {
        const isCollapsed = collapsedSections.has(prefix);
        return (
          <div className="file-tree-section" key={prefix}>
            <FileTreeFolderHeader
              isCollapsed={isCollapsed}
              toggleCollapsed={() =>
                setCollapsedSections(previous =>
                  previous.has(prefix)
                    ? new Set(Array.from(previous).filter(e => e !== prefix))
                    : new Set(Array.from(previous).concat(prefix)),
                )
              }
              folder={prefix}
            />
            {!isCollapsed ? <LinearFileList {...rest} files={files} /> : null}
          </div>
        );
      })}
    </div>
  );
}

/**
 * Display a list of changed files.
 *
 * (Case 1) If filesSubset is too long, but filesSubset.length === totalFiles, pagination buttons
 * are shown. This happens for uncommitted changes, where we have the entire list of files.
 *
 * (Case 2) If filesSubset.length < totalFiles, no pagination buttons are shown.
 * It's expected that filesSubset is already truncated to fit.
 * This happens initially for committed lists of changes, where we don't have the entire list of files.
 * Note that we later fetch the remaining files, to end up in (Case 1) again.
 *
 * In either case, a banner is shown to warn that not all files are shown.
 */
export function ChangedFiles(props: {
  filesSubset: ReadonlyArray<ChangedFile>;
  totalFiles: number;
  comparison: Comparison;
  selection?: UseUncommittedSelection;
  place?: Place;
}) {
  const displayType = useAtomValue(changedFilesDisplayType);
  const {filesSubset, totalFiles, ...rest} = props;
  const PAGE_SIZE = 500;
  const PAGE_FETCH_COUNT = 2;
  const [pageNum, setPageNum] = useState(0);
  const isLastPage = pageNum >= Math.floor((totalFiles - 1) / PAGE_SIZE);
  const rangeStart = pageNum * PAGE_SIZE;
  const rangeEnd = Math.min(filesSubset.length, (pageNum + 1) * PAGE_SIZE);
  const hasAdditionalPages = filesSubset.length > PAGE_SIZE;

  // We paginate files, but also paginate our fetches for generated statuses
  // at a larger granularity. This allows all manual files within that window
  // to be sorted to the front. This wider pagination is neceessary so we can control
  // how many files we query for generated statuses.
  const fetchPage = Math.floor(pageNum - (pageNum % PAGE_FETCH_COUNT));
  const fetchRangeStart = fetchPage * PAGE_SIZE;
  const fetchRangeEnd = (fetchPage + PAGE_FETCH_COUNT) * PAGE_SIZE;
  const filesToQueryGeneratedStatus = useMemo(
    () => filesSubset.slice(fetchRangeStart, fetchRangeEnd).map(f => f.path),
    [filesSubset, fetchRangeStart, fetchRangeEnd],
  );

  const generatedStatuses = useGeneratedFileStatuses(filesToQueryGeneratedStatus);
  const filesToSort = filesSubset.slice(fetchRangeStart, fetchRangeEnd);
  filesToSort.sort((a, b) => {
    const genStatA = generatedStatuses[a.path] ?? 0;
    const genStatB = generatedStatuses[b.path] ?? 0;
    return genStatA - genStatB;
  });
  const filesToShow = filesToSort.slice(rangeStart - fetchRangeStart, rangeEnd - fetchRangeStart);
  const processedFiles = useDeepMemo(() => processCopiesAndRenames(filesToShow), [filesToShow]);

  const prefixes: {key: string; prefix: string}[] = useMemo(
    () => Internal.repoPrefixes ?? [{key: 'default', prefix: ''}],
    [],
  );
  const firstNonDefaultPrefix = prefixes.find(
    p => p.prefix.length > 0 && filesToSort.some(f => f.path.indexOf(p.prefix) === 0),
  );
  const shouldShowRepoHeaders =
    prefixes.length > 1 &&
    firstNonDefaultPrefix != null &&
    filesToSort.find(f => f.path.indexOf(firstNonDefaultPrefix?.prefix) === -1) != null;

  const filesByPrefix = new Map<string, Array<UIChangedFile>>();
  for (const file of processedFiles) {
    for (const {key, prefix} of prefixes) {
      if (file.path.indexOf(prefix) === 0) {
        if (!filesByPrefix.has(key)) {
          filesByPrefix.set(key, []);
        }
        filesByPrefix.get(key)?.push(file);
        break;
      }
    }
  }

  useEffect(() => {
    // If the list of files is updated to have fewer files, we need to reset
    // the pageNum state to be in the proper range again.
    const lastPageIndex = Math.floor((totalFiles - 1) / PAGE_SIZE);
    if (pageNum > lastPageIndex) {
      setPageNum(Math.max(0, lastPageIndex));
    }
  }, [totalFiles, pageNum]);

  return (
    <div className="changed-files" data-testid="changed-files">
      {totalFiles > filesToShow.length ? (
        <Banner
          key={'alert'}
          icon={<Icon icon="info" />}
          buttons={
            hasAdditionalPages ? (
              <div className="changed-files-pages-buttons">
                <Tooltip title={t('See previous page of files')}>
                  <VSCodeButton
                    data-testid="changed-files-previous-page"
                    appearance="icon"
                    disabled={pageNum === 0}
                    onClick={() => {
                      setPageNum(old => old - 1);
                    }}>
                    <Icon icon="arrow-left" />
                  </VSCodeButton>
                </Tooltip>
                <Tooltip title={t('See next page of files')}>
                  <VSCodeButton
                    data-testid="changed-files-next-page"
                    appearance="icon"
                    disabled={isLastPage}
                    onClick={() => {
                      setPageNum(old => old + 1);
                    }}>
                    <Icon icon="arrow-right" />
                  </VSCodeButton>
                </Tooltip>
              </div>
            ) : null
          }>
          {pageNum === 0 ? (
            <T replace={{$numShown: filesToShow.length, $total: totalFiles}}>
              Showing first $numShown files out of $total total
            </T>
          ) : (
            <T replace={{$rangeStart: rangeStart + 1, $rangeEnd: rangeEnd, $total: totalFiles}}>
              Showing files $rangeStart – $rangeEnd out of $total total
            </T>
          )}
        </Banner>
      ) : null}
      {totalFiles > PAGE_SIZE * PAGE_FETCH_COUNT && (
        <Banner key="too-many-files" icon={<Icon icon="warning" />} kind={BannerKind.warning}>
          <T replace={{$maxFiles: PAGE_SIZE * PAGE_FETCH_COUNT}}>
            There are more than $maxFiles files, some files may appear out of order
          </T>
        </Banner>
      )}
      {displayType === 'tree' ? (
        <FileTree {...rest} files={processedFiles} displayType={displayType} />
      ) : shouldShowRepoHeaders ? (
        <SectionedFileList
          {...rest}
          filesByPrefix={filesByPrefix}
          displayType={displayType}
          generatedStatuses={generatedStatuses}
        />
      ) : (
        <LinearFileList
          {...rest}
          files={processedFiles}
          displayType={displayType}
          generatedStatuses={generatedStatuses}
        />
      )}
    </div>
  );
}

const generatedFilesInitiallyExpanded = localStorageBackedAtom<boolean>(
  'isl.expand-generated-files',
  false,
);

export const __TEST__ = {
  generatedFilesInitiallyExpanded,
};

function LinearFileList(props: {
  files: Array<UIChangedFile>;
  displayType: ChangedFilesDisplayType;
  generatedStatuses: Record<RepoRelativePath, GeneratedStatus>;
  comparison: Comparison;
  selection?: UseUncommittedSelection;
  place?: Place;
}) {
  const {files, generatedStatuses, ...rest} = props;

  const groupedByGenerated = group(files, file => generatedStatuses[file.path]);
  const [initiallyExpanded, setInitallyExpanded] = useAtom(generatedFilesInitiallyExpanded);

  function GeneratedFilesCollapsableSection(status: GeneratedStatus) {
    const group = groupedByGenerated[status] ?? [];
    if (group.length === 0) {
      return null;
    }
    return (
      <Collapsable
        title={
          <T
            replace={{
              $count: <Badge>{group.length}</Badge>,
            }}>
            {status === GeneratedStatus.PartiallyGenerated
              ? 'Partially Generated Files $count'
              : 'Generated Files $count'}
          </T>
        }
        startExpanded={status === GeneratedStatus.PartiallyGenerated || initiallyExpanded}
        onToggle={expanded => setInitallyExpanded(expanded)}>
        {group.map(file => (
          <File key={file.path} {...rest} file={file} generatedStatus={status} />
        ))}
      </Collapsable>
    );
  }

  return (
    <div className="changed-files-list-container">
      <div className="changed-files-list">
        {groupedByGenerated[GeneratedStatus.Manual]?.map(file => (
          <File
            key={file.path}
            {...rest}
            file={file}
            generatedStatus={generatedStatuses[file.path] ?? GeneratedStatus.Manual}
          />
        ))}
        {GeneratedFilesCollapsableSection(GeneratedStatus.PartiallyGenerated)}
        {GeneratedFilesCollapsableSection(GeneratedStatus.Generated)}
      </div>
    </div>
  );
}

export type Place = 'main' | 'amend sidebar' | 'commit sidebar';

export function UncommittedChanges({place}: {place: Place}) {
  const uncommittedChanges = useAtomValue(uncommittedChangesWithPreviews);
  const error = useAtomValue(uncommittedChangesFetchError);
  // TODO: use dagWithPreviews instead, and update CommitOperation
  const headCommit = useAtomValue(latestHeadCommit);
  const schema = useAtomValue(commitMessageFieldsSchema);
  const template = useAtomValue(commitMessageTemplate);

  const conflicts = useAtomValue(optimisticMergeConflicts);

  const selection = useUncommittedSelection();
  const commitTitleRef = useRef<HTMLTextAreaElement | undefined>(null);

  const runOperation = useRunOperation();

  const openCommitForm = useCallback(
    (which: 'commit' | 'amend') => {
      // make sure view is expanded
      writeAtom(islDrawerState, val => ({...val, right: {...val.right, collapsed: false}}));

      // show head commit & set to correct mode
      writeAtom(selectedCommits, new Set());
      writeAtom(commitMode, which);

      // Start editing fields when amending so you can go right into typing.
      if (which === 'amend') {
        writeAtom(forceNextCommitToEditAllFields, true);
        if (headCommit != null) {
          const latestMessage = readAtom(latestCommitMessageFields(headCommit.hash));
          if (latestMessage) {
            writeAtom(editedCommitMessages(headCommit.hash), {
              ...latestMessage,
            });
          }
        }
      }

      const quickCommitTyped = commitTitleRef.current?.value;
      if (which === 'commit' && quickCommitTyped != null && quickCommitTyped != '') {
        writeAtom(editedCommitMessages('head'), value => ({
          ...value,
          Title: quickCommitTyped,
        }));
        // delete what was written in the quick commit form
        commitTitleRef.current != null && (commitTitleRef.current.value = '');
      }
    },
    [headCommit],
  );

  const onConfirmQuickCommit = () => {
    const titleEl = commitTitleRef.current as HTMLInputElement | null;
    const title = titleEl?.value || template?.Title || temporaryCommitTitle();
    // use the template, unless a specific quick title is given
    const fields: CommitMessageFields = {...template, Title: title};
    const message = commitMessageFieldsToString(schema, fields);
    const hash = headCommit?.hash ?? '.';
    const allFiles = uncommittedChanges.map(file => file.path);
    const operation = getCommitOperation(message, hash, selection.selection, allFiles);
    selection.discardPartialSelections();
    runOperation(operation);
    if (titleEl) {
      // clear out message now that we've used it
      titleEl.value = '';
    }
  };

  if (error) {
    return <ErrorNotice title={t('Failed to fetch Uncommitted Changes')} error={error} />;
  }
  if (uncommittedChanges.length === 0 && conflicts == null) {
    return null;
  }
  const allFilesSelected = selection.isEverythingSelected();
  const noFilesSelected = selection.isNothingSelected();
  const hasChunkSelection = selection.hasChunkSelection();

  const allConflictsResolved =
    conflicts?.files?.every(conflict => conflict.status === 'Resolved') ?? false;

  // only show addremove button if some files are untracked/missing
  const UNTRACKED_OR_MISSING = ['?', '!'];
  const addremoveButton = uncommittedChanges.some(file =>
    UNTRACKED_OR_MISSING.includes(file.status),
  ) ? (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      title={t('Add all untracked files and remove all missing files.')}>
      <VSCodeButton
        appearance="icon"
        key="addremove"
        data-testid="addremove-button"
        onClick={() => {
          // If all files are selected, no need to pass specific files to addremove.
          const filesToAddRemove = allFilesSelected
            ? []
            : uncommittedChanges
                .filter(file => UNTRACKED_OR_MISSING.includes(file.status))
                .filter(file => selection.isFullyOrPartiallySelected(file.path))
                .map(file => file.path);
          runOperation(new AddRemoveOperation(filesToAddRemove));
        }}>
        <Icon slot="start" icon="expand-all" />
        <T>Add/Remove</T>
      </VSCodeButton>
    </Tooltip>
  ) : null;

  const onShelve = () => {
    const title = (commitTitleRef.current as HTMLInputElement | null)?.value || undefined;
    const allFiles = uncommittedChanges.map(file => file.path);
    const operation = getShelveOperation(title, selection.selection, allFiles);
    runOperation(operation);
  };

  const canAmend = headCommit && headCommit.phase !== 'public' && headCommit.successorInfo == null;

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
          <MergeConflictButtons allConflictsResolved={allConflictsResolved} conflicts={conflicts} />
        ) : (
          <>
            <ChangedFileDisplayTypePicker />
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
                selection.selectAll();
              }}>
              <Icon slot="start" icon="check-all" />
              <T>Select All</T>
            </VSCodeButton>
            <VSCodeButton
              appearance="icon"
              key="deselect-all"
              data-testid="deselect-all-button"
              disabled={noFilesSelected}
              onClick={() => {
                selection.deselectAll();
              }}>
              <Icon slot="start" icon="close-all" />
              <T>Deselect All</T>
            </VSCodeButton>
            {addremoveButton}
            <Tooltip
              delayMs={DOCUMENTATION_DELAY}
              title={t(
                'Discard selected uncommitted changes, including untracked files.\n\nNote: Changes will be irreversibly lost.',
              )}>
              <VSCodeButton
                appearance="icon"
                disabled={noFilesSelected}
                data-testid={'discard-all-selected-button'}
                onClick={() => {
                  platform.confirm(t('confirmDiscardChanges')).then(ok => {
                    if (!ok) {
                      return;
                    }
                    if (allFilesSelected) {
                      // all changes selected -> use clean goto rather than reverting each file. This is generally faster.

                      // to "discard", we need to both remove uncommitted changes
                      runOperation(new DiscardOperation());
                      // ...and delete untracked files.
                      // Technically we only need to do the purge when we have untracked files, though there's a chance there's files we don't know about yet while status is running.
                      runOperation(new PurgeOperation());
                    } else if (selection.hasChunkSelection()) {
                      // TODO(quark): Make PartialDiscardOperation replace the above and below cases.
                      const allFiles = uncommittedChanges.map(file => file.path);
                      const operation = new PartialDiscardOperation(selection.selection, allFiles);
                      selection.discardPartialSelections();
                      runOperation(operation);
                    } else {
                      const selectedFiles = uncommittedChanges.filter(file =>
                        selection.isFullyOrPartiallySelected(file.path),
                      );
                      const [selectedTrackedFiles, selectedUntrackedFiles] = partition(
                        selectedFiles,
                        file => file.status !== '?', // only untracked, not missing
                      );
                      if (selectedTrackedFiles.length > 0) {
                        // only a subset of files selected -> we need to revert selected tracked files individually
                        runOperation(new RevertOperation(selectedTrackedFiles.map(f => f.path)));
                      }
                      if (selectedUntrackedFiles.length > 0) {
                        // untracked files must be purged separately to delete from disk
                        runOperation(new PurgeOperation(selectedUntrackedFiles.map(f => f.path)));
                      }
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
      {conflicts != null ? (
        <ChangedFiles
          filesSubset={conflicts.files ?? []}
          totalFiles={conflicts.files?.length ?? 0}
          place={place}
          comparison={{
            type: ComparisonType.UncommittedChanges,
          }}
        />
      ) : (
        <ChangedFiles
          filesSubset={uncommittedChanges}
          totalFiles={uncommittedChanges.length}
          place={place}
          selection={selection}
          comparison={{
            type: ComparisonType.UncommittedChanges,
          }}
        />
      )}
      {conflicts != null || place !== 'main' ? null : (
        <div className="button-rows">
          <div className="button-row">
            <span className="quick-commit-inputs">
              <VSCodeButton
                appearance="icon"
                disabled={noFilesSelected}
                data-testid="quick-commit-button"
                onClick={onConfirmQuickCommit}>
                <Icon slot="start" icon="plus" />
                <T>Commit</T>
              </VSCodeButton>
              <VSCodeTextField
                data-testid="quick-commit-title"
                placeholder="Title"
                ref={commitTitleRef as MutableRefObject<null>}
                onKeyPress={e => {
                  if (e.key === 'Enter' && !(e.ctrlKey || e.metaKey || e.altKey || e.shiftKey)) {
                    onConfirmQuickCommit();
                  }
                }}
              />
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
            <Tooltip
              title={t(
                'Save selected uncommitted changes for later unshelving. Removes these changes from the working copy.',
              )}>
              <VSCodeButton
                disabled={noFilesSelected || hasChunkSelection}
                appearance="icon"
                className="show-on-hover"
                onClick={onShelve}>
                <Icon slot="start" icon="archive" />
                <T>Shelve</T>
              </VSCodeButton>
            </Tooltip>
          </div>
          {canAmend && (
            <div className="button-row">
              <VSCodeButton
                appearance="icon"
                disabled={noFilesSelected || !headCommit}
                data-testid="uncommitted-changes-quick-amend-button"
                onClick={() => {
                  const hash = headCommit?.hash ?? '.';
                  const allFiles = uncommittedChanges.map(file => file.path);
                  const operation = getAmendOperation(
                    undefined,
                    hash,
                    selection.selection,
                    allFiles,
                  );
                  selection.discardPartialSelections();
                  runOperation(operation);
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
          )}
        </div>
      )}
    </div>
  );
}

function MergeConflictButtons({
  conflicts,
  allConflictsResolved,
}: {
  conflicts: MergeConflicts;
  allConflictsResolved: boolean;
}) {
  const runOperation = useRunOperation();
  // usually we only care if the operation is queued or actively running,
  // but since we don't use optimistic state for continue/abort,
  // we also need to consider recently run commands to disable the buttons.
  // But only if the abort/continue command succeeded.
  // TODO: is this reliable? Is it possible to get stuck with buttons disabled because
  // we think it's still running?
  const lastRunOperation = useAtomValue(operationList).currentOperation;
  const justFinishedContinue =
    lastRunOperation?.operation instanceof ContinueOperation && lastRunOperation.exitCode === 0;
  const justFinishedAbort =
    lastRunOperation?.operation instanceof AbortMergeOperation && lastRunOperation.exitCode === 0;
  const isRunningContinue = !!useIsOperationRunningOrQueued(ContinueOperation);
  const isRunningAbort = !!useIsOperationRunningOrQueued(AbortMergeOperation);
  const shouldDisableButtons =
    isRunningContinue || isRunningAbort || justFinishedContinue || justFinishedAbort;

  return (
    <>
      <VSCodeButton
        appearance={allConflictsResolved ? 'primary' : 'icon'}
        key="continue"
        disabled={!allConflictsResolved || shouldDisableButtons}
        data-testid="conflict-continue-button"
        onClick={() => {
          runOperation(new ContinueOperation());
        }}>
        <Icon slot="start" icon={isRunningContinue ? 'loading' : 'debug-continue'} />
        <T>Continue</T>
      </VSCodeButton>
      <VSCodeButton
        appearance="icon"
        key="abort"
        disabled={shouldDisableButtons}
        onClick={() => {
          runOperation(new AbortMergeOperation(conflicts));
        }}>
        <Icon slot="start" icon={isRunningAbort ? 'loading' : 'circle-slash'} />
        <T>Abort</T>
      </VSCodeButton>
    </>
  );
}
