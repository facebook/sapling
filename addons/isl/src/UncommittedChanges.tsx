/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitMessageFields} from './CommitInfoView/types';
import type {UseUncommittedSelection} from './partialSelection';
import type {ChangedFile, ChangedFileType, MergeConflicts, RepoRelativePath} from './types';
import type {MutableRefObject, ReactNode} from 'react';
import type {Comparison} from 'shared/Comparison';

import {Banner, BannerKind} from './Banner';
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
import {
  generatedStatusToLabel,
  generatedStatusDescription,
  useGeneratedFileStatuses,
} from './GeneratedFile';
import {Internal} from './Internal';
import {PartialFileSelectionWithMode} from './PartialFileSelection';
import {SuspenseBoundary} from './SuspenseBoundary';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {latestCommitMessageFields} from './codeReview/CodeReviewInfo';
import {islDrawerState} from './drawerState';
import {T, t} from './i18n';
import {AbortMergeOperation} from './operations/AbortMergeOperation';
import {AddOperation} from './operations/AddOperation';
import {AddRemoveOperation} from './operations/AddRemoveOperation';
import {getAmendOperation} from './operations/AmendOperation';
import {getCommitOperation} from './operations/CommitOperation';
import {ContinueOperation} from './operations/ContinueMergeOperation';
import {DiscardOperation, PartialDiscardOperation} from './operations/DiscardOperation';
import {ForgetOperation} from './operations/ForgetOperation';
import {PurgeOperation} from './operations/PurgeOperation';
import {ResolveOperation, ResolveTool} from './operations/ResolveOperation';
import {RevertOperation} from './operations/RevertOperation';
import {getShelveOperation} from './operations/ShelveOperation';
import {useUncommittedSelection} from './partialSelection';
import platform from './platform';
import {
  optimisticMergeConflicts,
  uncommittedChangesWithPreviews,
  useIsOperationRunningOrQueued,
} from './previews';
import {selectedCommits} from './selection';
import {
  latestHeadCommit,
  operationList,
  uncommittedChangesFetchError,
  useRunOperation,
} from './serverAPIState';
import {useToast} from './toast';
import {succeedableRevset, GeneratedStatus} from './types';
import {usePromise} from './usePromise';
import {
  VSCodeBadge,
  VSCodeButton,
  VSCodeCheckbox,
  VSCodeTextField,
} from '@vscode/webview-ui-toolkit/react';
import React, {useMemo, useEffect, useRef, useState} from 'react';
import {atom, useRecoilCallback, useRecoilValue} from 'recoil';
import {labelForComparison, revsetForComparison, ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {isMac} from 'shared/OperatingSystem';
import {useDeepMemo} from 'shared/hooks';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {basename, group, notEmpty, partition} from 'shared/utils';

import './UncommittedChanges.css';

const platformAltKey = (e: KeyboardEvent) => (isMac ? e.altKey : e.ctrlKey);
/**
 * Is the alt key currently held down, used to show full file paths.
 * On windows, this actually uses the ctrl key instead to avoid conflicting with OS focus behaviors.
 */
const holdingAltAtom = atom<boolean>({
  key: 'holdingAltAtom',
  default: false,
  effects: [
    ({setSelf}) => {
      const keydown = (e: KeyboardEvent) => {
        if (platformAltKey(e)) {
          setSelf(true);
        }
      };
      const keyup = (e: KeyboardEvent) => {
        if (!platformAltKey(e)) {
          setSelf(false);
        }
      };
      document.addEventListener('keydown', keydown);
      document.addEventListener('keyup', keyup);
      return () => {
        document.removeEventListener('keydown', keydown);
        document.removeEventListener('keyup', keyup);
      };
    },
  ],
});

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

type VisualChangedFileType = ChangedFileType | 'Renamed' | 'Copied';

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
  filesSubset: Array<ChangedFile>;
  totalFiles: number;
  comparison: Comparison;
  selection?: UseUncommittedSelection;
  place?: Place;
}) {
  const displayType = useRecoilValue(changedFilesDisplayType);
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
              $count: <VSCodeBadge>{group.length}</VSCodeBadge>,
            }}>
            {status === GeneratedStatus.PartiallyGenerated
              ? 'Partially Generated Files $count'
              : 'Generated Files $count'}
          </T>
        }
        startExpanded={status === GeneratedStatus.PartiallyGenerated}>
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
  const toast = useToast();
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
  const isHoldingAlt = useRecoilValue(holdingAltAtom);

  const tooltip = file.tooltip + '\n\n' + generatedStatusDescription(generatedStatus);

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

export type Place = 'main' | 'amend sidebar' | 'commit sidebar';

export function UncommittedChanges({place}: {place: Place}) {
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const error = useRecoilValue(uncommittedChangesFetchError);
  // TODO: use dagWithPreviews instead, and update CommitOperation
  const headCommit = useRecoilValue(latestHeadCommit);
  const schema = useRecoilValue(commitMessageFieldsSchema);
  const template = useRecoilValue(commitMessageTemplate);

  const conflicts = useRecoilValue(optimisticMergeConflicts);

  const selection = useUncommittedSelection();
  const commitTitleRef = useRef<HTMLTextAreaElement | undefined>(null);

  const runOperation = useRunOperation();

  const openCommitForm = useRecoilCallback(
    ({set, reset, snapshot}) =>
      (which: 'commit' | 'amend') => {
        // make sure view is expanded
        set(islDrawerState, val => ({...val, right: {...val.right, collapsed: false}}));

        // show head commit & set to correct mode
        reset(selectedCommits);
        set(commitMode, which);

        // Start editing fields when amending so you can go right into typing.
        if (which === 'amend') {
          set(forceNextCommitToEditAllFields, true);
          if (headCommit != null) {
            const latestMessage = snapshot
              .getLoadable(latestCommitMessageFields(headCommit.hash))
              .valueMaybe();
            if (latestMessage) {
              set(editedCommitMessages(headCommit.hash), {
                fields: {...latestMessage},
              });
            }
          }
        }

        const quickCommitTyped = commitTitleRef.current?.value;
        if (which === 'commit' && quickCommitTyped != null && quickCommitTyped != '') {
          set(editedCommitMessages('head'), value => ({
            ...value,
            fields: {...value.fields, Title: quickCommitTyped},
          }));
          // delete what was written in the quick commit form
          commitTitleRef.current != null && (commitTitleRef.current.value = '');
        }
      },
  );

  const onConfirmQuickCommit = () => {
    const title =
      (commitTitleRef.current as HTMLInputElement | null)?.value ||
      template?.fields.Title ||
      temporaryCommitTitle();
    // use the template, unless a specific quick title is given
    const fields: CommitMessageFields = {...template?.fields, Title: title};
    const message = commitMessageFieldsToString(schema, fields);
    const hash = headCommit?.hash ?? '.';
    const allFiles = uncommittedChanges.map(file => file.path);
    const operation = getCommitOperation(message, hash, selection.selection, allFiles);
    selection.discardPartialSelections();
    runOperation(operation);
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
          {headCommit?.phase === 'public' ? null : (
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
  const lastRunOperation = useRecoilValue(operationList).currentOperation;
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
  const conflicts = useRecoilValue(optimisticMergeConflicts);

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
          <Icon icon="git-pull-request-go-to-changes" />
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
