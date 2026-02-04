/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';
import type {ParsedDiff} from 'shared/patch/types';
import type {Result} from '../types';
import type {Context} from './SplitDiffView/types';

import deepEqual from 'fast-deep-equal';
import {Button} from 'isl-components/Button';
import {Dropdown} from 'isl-components/Dropdown';
import {ErrorBoundary, ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {RadioGroup} from 'isl-components/Radio';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue, useSetAtom} from 'jotai';
import {useCallback, useEffect, useMemo, useRef, useState} from 'react';
import {
  ComparisonType,
  comparisonIsAgainstHead,
  comparisonStringKey,
  labelForComparison,
} from 'shared/Comparison';
import {group, notEmpty} from 'shared/utils';
import serverAPI from '../ClientToServerAPI';
import {EmptyState} from '../EmptyState';
import {useGeneratedFileStatuses} from '../GeneratedFile';
import {T, t} from '../i18n';
import {atomFamilyWeak, atomLoadableWithRefresh, localStorageBackedAtom} from '../jotaiUtils';
import platform from '../platform';
import {
  pendingCommentsAtom,
  CommentInput,
  PendingCommentDisplay,
  PendingCommentsBadge,
} from '../reviewComments';
import {latestHeadCommit} from '../serverAPIState';
import {reviewModeAtom} from '../reviewMode';
import {MergeControls} from '../reviewMode/MergeControls';
import {useSubmitReview, useQuickReviewAction} from '../reviewSubmission';
import {allDiffSummaries} from '../codeReview/CodeReviewInfo';
import {themeState} from '../theme';
import {GeneratedStatus} from '../types';
import {SplitDiffView} from './SplitDiffView';
import {currentComparisonMode, reviewedFilesAtom, reviewedFileKey, reviewedFileKeyForPR} from './atoms';
import {parsePatchAndFilter, sortFilesByType} from './utils';
import {SyncPRButton} from './SyncPRButton';
import {SyncProgress} from './SyncProgress';
import {currentPRStackContextAtom} from '../codeReview/PRStacksAtom';
import {enterReviewMode} from '../reviewMode';
import {diffSummary} from '../codeReview/CodeReviewInfo';

import './ComparisonView.css';

/**
 * Transform Result<T> to Result<U> by applying `fn` on result.value.
 * If the result is an error, just return it unchanged.
 */
function mapResult<T, U>(result: Result<T>, fn: (t: T) => U): Result<U> {
  return result.error == null ? {value: fn(result.value)} : result;
}

const currentComparisonData = atomFamilyWeak((comparison: Comparison) =>
  atomLoadableWithRefresh<Result<Array<ParsedDiff>>>(async () => {
    serverAPI.postMessage({type: 'requestComparison', comparison});
    const event = await serverAPI.nextMessageMatching('comparison', event =>
      deepEqual(comparison, event.comparison),
    );
    return mapResult(event.data.diff, parsePatchAndFilter);
  }),
);

/**
 * PR title and description header shown in review mode.
 */
function PRInfoHeader({prNumber}: {prNumber: string}) {
  const prData = useAtomValue(diffSummary(prNumber));
  const pr = prData?.value;

  if (!pr) {
    return null;
  }

  const title = pr.title || `PR #${prNumber}`;
  const description = pr.type === 'github' ? pr.body : undefined;
  const prUrl = pr.type === 'github' ? pr.url : null;

  return (
    <div className="pr-info-header">
      <div className="pr-info-title-row">
        <span className="pr-info-number">#{prNumber}</span>
        {prUrl ? (
          <a
            href={prUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="pr-info-title"
            title={title}
          >
            {title}
          </a>
        ) : (
          <span className="pr-info-title" title={title}>{title}</span>
        )}
      </div>
      {description && (
        <div className="pr-info-description" title={description}>
          {description.length > 120 ? `${description.slice(0, 120)}...` : description}
        </div>
      )}
    </div>
  );
}

/**
 * Hook to enforce minimum display time for loading states.
 * Once loading starts, keeps returning true for at least minTime ms
 * to prevent flickery skeleton flashes.
 */
function useMinimumLoadingTime(isLoading: boolean, minTime: number = 300): boolean {
  const [showLoading, setShowLoading] = useState(isLoading);
  const loadingStartRef = useRef<number | null>(null);

  useEffect(() => {
    if (isLoading) {
      // Started loading - record the time
      if (loadingStartRef.current === null) {
        loadingStartRef.current = Date.now();
      }
      setShowLoading(true);
    } else {
      // Finished loading - ensure minimum display time
      if (loadingStartRef.current !== null) {
        const elapsed = Date.now() - loadingStartRef.current;
        const remaining = minTime - elapsed;

        if (remaining > 0) {
          const timer = setTimeout(() => {
            setShowLoading(false);
            loadingStartRef.current = null;
          }, remaining);
          return () => clearTimeout(timer);
        } else {
          setShowLoading(false);
          loadingStartRef.current = null;
        }
      } else {
        setShowLoading(false);
      }
    }
  }, [isLoading, minTime]);

  return showLoading;
}

/**
 * Skeleton loading state for diff files.
 * Shows placeholder file headers and diff lines while loading.
 */
function DiffSkeleton({fileCount = 3}: {fileCount?: number}) {
  // Varying line counts and widths for natural look
  const fileConfigs = [
    {lines: 8, pathWidth: 220},
    {lines: 5, pathWidth: 180},
    {lines: 12, pathWidth: 260},
  ];

  const lineWidths = [65, 45, 80, 30, 55, 70, 40, 60, 50, 75, 35, 85];

  return (
    <div className="skeleton-files-container">
      {Array.from({length: fileCount}).map((_, fileIdx) => {
        const config = fileConfigs[fileIdx % fileConfigs.length];
        return (
          <div key={fileIdx} className="skeleton-file">
            {/* File header skeleton */}
            <div className="skeleton-file-header">
              <div className="skeleton-checkbox skeleton-shimmer" />
              <div className="skeleton-chevron skeleton-shimmer" />
              <div className="skeleton-file-icon skeleton-shimmer" />
              <div
                className="skeleton-file-path skeleton-shimmer"
                style={{width: config.pathWidth}}
              />
              <div className="skeleton-file-actions">
                <div className="skeleton-action-btn skeleton-shimmer" />
                <div className="skeleton-action-btn skeleton-shimmer" />
              </div>
            </div>
            {/* Diff content skeleton */}
            <div className="skeleton-diff-content">
              {Array.from({length: config.lines}).map((_, lineIdx) => (
                <div key={lineIdx} className="skeleton-diff-row">
                  <div className="skeleton-line-number">
                    <div className="skeleton-line-number-bar skeleton-shimmer" />
                  </div>
                  <div className="skeleton-line-content">
                    <div
                      className="skeleton-code-bar skeleton-shimmer"
                      style={{width: `${lineWidths[lineIdx % lineWidths.length]}%`}}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Horizontal bar showing all PRs in a stack for navigation.
 * Shows stack direction: left = base (closest to main), right = tip (newest).
 * Only renders when in review mode with a multi-PR stack.
 */
function StackNavigationBar() {
  const stackContext = useAtomValue(currentPRStackContextAtom);
  const reviewMode = useAtomValue(reviewModeAtom);

  // Always render PR info if in review mode with a PR number
  const showPRInfo = reviewMode.active && reviewMode.prNumber;

  // Don't render stack nav if single PR
  const showStackNav = stackContext && !stackContext.isSinglePr;

  if (!showPRInfo && !showStackNav) {
    return null;
  }

  const handleNavigateToPR = (prNumber: number, headHash: string) => {
    // Skip navigation if headHash is empty (PR not in summaries)
    if (!headHash) {
      return;
    }
    enterReviewMode(String(prNumber), headHash);
  };

  // Reverse entries so base (closest to main) is on left, tip (newest) is on right
  const reversedEntries = stackContext ? [...stackContext.entries].reverse() : [];
  // Calculate position from reversed perspective (1-indexed from base)
  const positionFromBase = stackContext ? stackContext.stackSize - stackContext.currentIndex : 0;

  return (
    <div className="stack-navigation-container">
      {/* PR Info - always show in review mode */}
      {showPRInfo && <PRInfoHeader prNumber={reviewMode.prNumber!} />}

      {/* Stack Navigation - only for multi-PR stacks */}
      {showStackNav && (
        <div className="stack-navigation-bar">
          <span className="stack-label">
            <T>Stack</T>
          </span>
          <span className="stack-direction-hint stack-direction-base">
            <T>main</T>
            <Icon icon="arrow-right" />
          </span>
          <div className="stack-pr-pills">
            {reversedEntries.map((entry, idx) => {
              // Determine review status class
              const reviewClass = entry.reviewDecision === 'APPROVED'
                ? 'stack-pr-approved'
                : entry.reviewDecision === 'CHANGES_REQUESTED'
                  ? 'stack-pr-changes-requested'
                  : '';

              const pill = (
                <Tooltip
                  key={entry.prNumber}
                  title={entry.title}
                  delayMs={500}
                >
                  <Button
                    className={`stack-pr-pill ${entry.isCurrent ? 'stack-pr-current' : ''} ${entry.state === 'MERGED' ? 'stack-pr-merged' : ''} ${reviewClass}`}
                    onClick={() => handleNavigateToPR(entry.prNumber, entry.headHash)}
                    disabled={entry.isCurrent || !entry.headHash}
                  >
                    {entry.reviewDecision === 'APPROVED' && !entry.isCurrent && <Icon icon="check" />}
                    {entry.reviewDecision === 'CHANGES_REQUESTED' && !entry.isCurrent && <Icon icon="diff" />}
                    #{entry.prNumber}
                    {entry.state === 'MERGED' && <Icon icon="git-merge" />}
                  </Button>
                </Tooltip>
              );
              return idx > 0 ? (
                <span key={entry.prNumber} className="stack-pill-with-arrow">
                  <span className="stack-arrow"><Icon icon="arrow-right" /></span>
                  {pill}
                </span>
              ) : pill;
            })}
          </div>
          <span className="stack-position">
            {positionFromBase} / {stackContext!.stackSize}
          </span>
        </div>
      )}
    </div>
  );
}

type LineRangeKey = string;
export function keyForLineRange(param: {path: string; comparison: Comparison}): LineRangeKey {
  return `${param.path}:${comparisonStringKey(param.comparison)}`;
}

type ComparisonDisplayMode = 'unified' | 'split';
const comparisonDisplayMode = localStorageBackedAtom<ComparisonDisplayMode | 'responsive'>(
  'isl.comparison-display-mode',
  'responsive',
);

export default function ComparisonView({
  comparison,
  dismiss,
  scrollToFile,
}: {
  comparison: Comparison;
  dismiss?: () => void;
  scrollToFile?: string;
}) {
  const compared = useAtomValue(currentComparisonData(comparison));

  const displayMode = useComparisonDisplayMode();

  // Enforce minimum display time for skeleton to prevent flickery loading
  const isActuallyLoading = compared.state === 'loading';
  const showSkeleton = useMinimumLoadingTime(isActuallyLoading, 300);

  const data = !showSkeleton && compared.state === 'hasData' ? compared.data : null;

  const paths = useMemo(
    () => data?.value?.map(file => file.newFileName).filter(notEmpty) ?? [],
    [data?.value],
  );
  const generatedStatuses = useGeneratedFileStatuses(paths);
  const [collapsedFiles, setCollapsedFile] = useCollapsedFilesState({
    isLoading: isActuallyLoading,
    data: compared.state === 'hasData' ? compared.data : null,
  });

  // File navigation state for review mode
  const [currentFileIndex, setCurrentFileIndex] = useState(0);
  const reviewMode = useAtomValue(reviewModeAtom);

  // State for PR-level comment input
  const [showPrComment, setShowPrComment] = useState(false);

  // Get list of file paths for navigation
  const filePaths = useMemo(
    () =>
      data?.value?.map(file => file.newFileName ?? file.oldFileName ?? '').filter(Boolean) ?? [],
    [data?.value],
  );

  // Refs for scrolling to specific files
  const fileRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const setFileRef = useCallback((path: string, element: HTMLDivElement | null) => {
    if (element) {
      fileRefs.current.set(path, element);
    } else {
      fileRefs.current.delete(path);
    }
  }, []);

  const handleNextFile = useCallback(() => {
    if (currentFileIndex < filePaths.length - 1) {
      const nextIndex = currentFileIndex + 1;
      setCurrentFileIndex(nextIndex);
      const path = filePaths[nextIndex];
      const element = fileRefs.current.get(path);
      if (element) {
        element.scrollIntoView({behavior: 'smooth', block: 'start'});
        // Expand if collapsed
        if (collapsedFiles.get(path)) {
          setCollapsedFile(path, false);
        }
      }
    }
  }, [currentFileIndex, filePaths, collapsedFiles, setCollapsedFile]);

  const handlePrevFile = useCallback(() => {
    if (currentFileIndex > 0) {
      const prevIndex = currentFileIndex - 1;
      setCurrentFileIndex(prevIndex);
      const path = filePaths[prevIndex];
      const element = fileRefs.current.get(path);
      if (element) {
        element.scrollIntoView({behavior: 'smooth', block: 'start'});
        // Expand if collapsed
        if (collapsedFiles.get(path)) {
          setCollapsedFile(path, false);
        }
      }
    }
  }, [currentFileIndex, filePaths, collapsedFiles, setCollapsedFile]);

  // Scroll to file when scrollToFile is set and data is loaded
  useEffect(() => {
    if (scrollToFile && data?.value) {
      // Small delay to ensure the DOM has rendered
      const timer = setTimeout(() => {
        const element = fileRefs.current.get(scrollToFile);
        if (element) {
          element.scrollIntoView({behavior: 'smooth', block: 'start'});
          // Expand the file if it's collapsed
          if (collapsedFiles.get(scrollToFile)) {
            setCollapsedFile(scrollToFile, false);
          }
        }
      }, 100);
      return () => clearTimeout(timer);
    }
  }, [scrollToFile, data?.value, collapsedFiles, setCollapsedFile]);

  let content;
  if (data == null) {
    content = <DiffSkeleton />;
  } else if (compared.state === 'hasError') {
    const error = compared.error instanceof Error ? compared.error : new Error(`${compared.error}`);
    content = <ErrorNotice error={error} title={t('Unable to load comparison')} />;
  } else if (data?.value && data.value.length === 0) {
    content =
      comparison.type === ComparisonType.SinceLastCodeReviewSubmit ? (
        <EmptyState>
          <T>No Content Changes</T>
          <br />
          <Subtle>
            <T> This commit might have been rebased</T>
          </Subtle>
        </EmptyState>
      ) : (
        <EmptyState>
          <T>No Changes</T>
        </EmptyState>
      );
  } else {
    const files = data.value ?? [];
    sortFilesByType(files);
    const fileGroups = group(files, file => generatedStatuses[file.newFileName ?? '']);
    content = (
      <>
        {fileGroups[GeneratedStatus.Manual]?.map((parsed, i) => (
          <ComparisonViewFile
            diff={parsed}
            comparison={comparison}
            key={i}
            collapsed={collapsedFiles.get(parsed.newFileName ?? '') ?? false}
            setCollapsed={(collapsed: boolean) =>
              setCollapsedFile(parsed.newFileName ?? '', collapsed)
            }
            generatedStatus={GeneratedStatus.Manual}
            displayMode={displayMode}
            setRef={setFileRef}
          />
        ))}
        {fileGroups[GeneratedStatus.PartiallyGenerated]?.map((parsed, i) => (
          <ComparisonViewFile
            diff={parsed}
            comparison={comparison}
            key={i}
            collapsed={collapsedFiles.get(parsed.newFileName ?? '') ?? false}
            setCollapsed={(collapsed: boolean) =>
              setCollapsedFile(parsed.newFileName ?? '', collapsed)
            }
            generatedStatus={GeneratedStatus.PartiallyGenerated}
            displayMode={displayMode}
            setRef={setFileRef}
          />
        ))}
        {fileGroups[GeneratedStatus.Generated]?.map((parsed, i) => (
          <ComparisonViewFile
            diff={parsed}
            comparison={comparison}
            key={i}
            collapsed={collapsedFiles.get(parsed.newFileName ?? '') ?? false}
            setCollapsed={(collapsed: boolean) =>
              setCollapsedFile(parsed.newFileName ?? '', collapsed)
            }
            generatedStatus={GeneratedStatus.Generated}
            displayMode={displayMode}
            setRef={setFileRef}
          />
        ))}
      </>
    );
  }

  return (
    <div data-testid="comparison-view" className="comparison-view">
      {/* Scrollable container - holds sticky header + content */}
      <div className="comparison-view-scrollable">
        {/* Sticky header: header + stack navigation */}
        <div className="comparison-view-sticky-header">
          <ComparisonViewHeader
            comparison={comparison}
            collapsedFiles={collapsedFiles}
            setCollapsedFile={setCollapsedFile}
            dismiss={dismiss}
            currentFileIndex={currentFileIndex}
            totalFiles={filePaths.length}
            onPrevFile={handlePrevFile}
            onNextFile={handleNextFile}
            showNavigation={reviewMode.active && filePaths.length > 1}
          />
          <StackNavigationBar />
        </div>
        {/* Content area */}
        <div className="comparison-view-details">
          {content}
          {/* Review actions at bottom - only shown in review mode with a PR */}
          {reviewMode.active && reviewMode.prNumber && (
            <div className="comparison-view-merge-section">
              {/* PR-level comment input */}
              {showPrComment && (
                <div className="pr-level-comments">
                  <div className="pr-level-comments-header">
                    <span className="pr-level-comments-title">
                      <T>Review comment</T>
                    </span>
                  </div>
                  <CommentInput
                    prNumber={reviewMode.prNumber}
                    type="pr"
                    onCancel={() => setShowPrComment(false)}
                  />
                </div>
              )}
              {/* Add comment button and pending badge */}
              <div className="review-mode-footer">
                <PendingCommentsBadge prNumber={reviewMode.prNumber} />
                <Button icon onClick={() => setShowPrComment(true)}>
                  <Icon icon="comment" slot="start" />
                  <T>Add review comment</T>
                </Button>
              </div>
              <MergeControls key={reviewMode.prNumber} prNumber={reviewMode.prNumber} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

const defaultComparisons = [
  ComparisonType.UncommittedChanges as const,
  ComparisonType.HeadChanges as const,
  ComparisonType.StackChanges as const,
];
function ComparisonViewHeader({
  comparison,
  collapsedFiles,
  setCollapsedFile,
  dismiss,
  currentFileIndex,
  totalFiles,
  onPrevFile,
  onNextFile,
  showNavigation,
}: {
  comparison: Comparison;
  collapsedFiles: Map<string, boolean>;
  setCollapsedFile: (path: string, collapsed: boolean) => unknown;
  dismiss?: () => void;
  currentFileIndex?: number;
  totalFiles?: number;
  onPrevFile?: () => void;
  onNextFile?: () => void;
  showNavigation?: boolean;
}) {
  const setComparisonMode = useSetAtom(currentComparisonMode);
  const [compared, reloadComparison] = useAtom(currentComparisonData(comparison));

  const data = compared.state === 'hasData' ? compared.data : null;

  const allFilesExpanded =
    data?.value?.every(
      file => file.newFileName && collapsedFiles.get(file.newFileName) === false,
    ) === true;
  const noFilesExpanded =
    data?.value?.every(
      file => file.newFileName && collapsedFiles.get(file.newFileName) === true,
    ) === true;
  const isLoading = compared.state === 'loading';

  // Review mode: Get nodeId from diff summaries for Submit Review button
  const reviewMode = useAtomValue(reviewModeAtom);
  const allDiffs = useAtomValue(allDiffSummaries);

  // Get nodeId from diff summaries when in review mode
  const nodeId = useMemo(() => {
    if (!reviewMode.active || !reviewMode.prNumber) return undefined;
    const summaries = allDiffs.value;
    if (!summaries) return undefined;
    const summary = summaries.get(reviewMode.prNumber);
    if (summary?.type !== 'github') return undefined;
    return summary.nodeId;
  }, [reviewMode.active, reviewMode.prNumber, allDiffs]);

  const {submitReview, canSubmit, pendingCommentCount} = useSubmitReview(nodeId);
  const {approve, requestChanges, canSubmit: canQuickSubmit} = useQuickReviewAction(nodeId);

  return (
    <>
      <div className="comparison-view-header">
        <span className="comparison-view-header-group">
          <Dropdown
            data-testid="comparison-view-picker"
            value={comparison.type}
            onChange={event => {
              const newComparison = {
                type: (event as React.FormEvent<HTMLSelectElement>).currentTarget
                  .value as (typeof defaultComparisons)[0],
              };
              setComparisonMode(previous => ({
                ...previous,
                comparison: newComparison,
              }));
              // When viewed in a dedicated viewer, change the title as the comparison changes
              if (window.islAppMode != null && window.islAppMode.mode != 'isl') {
                serverAPI.postMessage({
                  type: 'platform/changeTitle',
                  title: labelForComparison(newComparison),
                });
              }
            }}
            options={[
              ...defaultComparisons.map(comparison => ({
                value: comparison,
                name: labelForComparison({type: comparison}),
              })),

              !defaultComparisons.includes(comparison.type as (typeof defaultComparisons)[0])
                ? {value: comparison.type, name: labelForComparison(comparison)}
                : undefined,
            ].filter(notEmpty)}
          />
          <Tooltip
            delayMs={1000}
            title={t('Reload this comparison. Comparisons do not refresh automatically.')}>
            <Button onClick={reloadComparison}>
              <Icon icon="refresh" data-testid="comparison-refresh-button" />
            </Button>
          </Tooltip>
          {showNavigation && totalFiles != null && totalFiles > 0 && (
            <span className="comparison-view-file-navigation">
              <Button
                icon
                onClick={onPrevFile}
                disabled={currentFileIndex === 0}
                data-testid="prev-file-button">
                <Icon icon="arrow-up" />
              </Button>
              <span className="file-nav-indicator">
                {(currentFileIndex ?? 0) + 1} / {totalFiles}
              </span>
              <Button
                icon
                onClick={onNextFile}
                disabled={currentFileIndex === (totalFiles ?? 1) - 1}
                data-testid="next-file-button">
                <Icon icon="arrow-down" />
              </Button>
            </span>
          )}
          {/* Quick review action buttons */}
          {reviewMode.active && (
            <>
              <Button
                className="review-action-btn review-action-approve"
                onClick={approve}
                disabled={!canQuickSubmit}
                data-testid="quick-approve-button">
                <Icon icon="check" slot="start" />
                <T>Approved</T>
              </Button>
              <Button
                className="review-action-btn review-action-request-changes"
                onClick={requestChanges}
                disabled={!canQuickSubmit}
                data-testid="quick-request-changes-button">
                <Icon icon="diff" slot="start" />
                <T>Request Changes</T>
              </Button>
            </>
          )}
          <Button
            onClick={() => {
              for (const file of data?.value ?? []) {
                if (file.newFileName) {
                  setCollapsedFile(file.newFileName, false);
                }
              }
            }}
            disabled={isLoading || allFilesExpanded}
            icon>
            <Icon icon="unfold" slot="start" />
            <T>Expand all files</T>
          </Button>
          <Button
            onClick={() => {
              for (const file of data?.value ?? []) {
                if (file.newFileName) {
                  setCollapsedFile(file.newFileName, true);
                }
              }
            }}
            icon
            disabled={isLoading || noFilesExpanded}>
            <Icon icon="fold" slot="start" />
            <T>Collapse all files</T>
          </Button>
          <Tooltip trigger="click" component={() => <ComparisonSettingsDropdown />}>
            <Button icon>
              <Icon icon="ellipsis" />
            </Button>
          </Tooltip>
          {isLoading ? <Icon icon="loading" data-testid="comparison-loading" /> : null}
        </span>
        {dismiss == null ? null : (
          <Button data-testid="close-comparison-view-button" icon onClick={dismiss}>
            <Icon icon="x" />
          </Button>
        )}
      </div>
    </>
  );
}

function ComparisonSettingsDropdown() {
  const [mode, setMode] = useAtom(comparisonDisplayMode);
  return (
    <div className="dropdown-field">
      <RadioGroup
        title={t('Comparison Display Mode')}
        choices={[
          {value: 'responsive', title: <T>Responsive</T>},
          {value: 'split', title: <T>Split</T>},
          {value: 'unified', title: <T>Unified</T>},
        ]}
        current={mode}
        onChange={setMode}
      />
    </div>
  );
}

/**
 * Derive from the parsed diff state which files should be expanded or collapsed by default.
 * This state is the source of truth of which files are expanded/collapsed.
 * This is a hook instead of a recoil selector since it depends on the comparison
 * which is a prop.
 */
function useCollapsedFilesState(data: {
  isLoading: boolean;
  data: Result<Array<ParsedDiff>> | null;
}): [Map<string, boolean>, (path: string, collapsed: boolean) => void] {
  const [collapsedFiles, setCollapsedFiles] = useState(new Map());

  useEffect(() => {
    if (data.isLoading || data.data?.value == null) {
      return;
    }

    const newCollapsedFiles = new Map(collapsedFiles);

    // Allocate a number of changed lines we're willing to show expanded by default,
    // add files until we just cross that threshold.
    // This means a single very large file will start expanded already.
    const TOTAL_DEFAULT_EXPANDED_SIZE = 4000;
    let accumulatedSize = 0;
    let indexToStartCollapsing = Infinity;
    for (const [i, diff] of data.data.value.entries()) {
      const sizeThisFile = diff.hunks.reduce((last, hunk) => last + hunk.lines.length, 0);
      accumulatedSize += sizeThisFile;
      if (accumulatedSize > TOTAL_DEFAULT_EXPANDED_SIZE) {
        indexToStartCollapsing = i;
        break;
      }
    }

    let anyChanged = false;
    for (const [i, diff] of data.data.value.entries()) {
      if (!newCollapsedFiles.has(diff.newFileName)) {
        newCollapsedFiles.set(diff.newFileName, i > 0 && i >= indexToStartCollapsing);
        anyChanged = true;
      }
      // Leave existing files alone in case the user changed their expanded state.
    }
    if (anyChanged) {
      setCollapsedFiles(newCollapsedFiles);
      // We don't bother removing files that no longer appear in the list of files.
      // That's not a big deal, this state is local to this instance of the comparison view anyway.
    }
  }, [data, collapsedFiles]);

  const setCollapsed = (path: string, collapsed: boolean) => {
    setCollapsedFiles(prev => {
      const map = new Map(prev);
      map.set(path, collapsed);
      return map;
    });
  };

  return [collapsedFiles, setCollapsed];
}

function splitOrUnifiedBasedOnWidth() {
  return window.innerWidth > 600 ? 'split' : 'unified';
}
function useComparisonDisplayMode(): ComparisonDisplayMode {
  const underlyingMode = useAtomValue(comparisonDisplayMode);
  const [mode, setMode] = useState(
    underlyingMode === 'responsive' ? splitOrUnifiedBasedOnWidth() : underlyingMode,
  );
  useEffect(() => {
    if (underlyingMode !== 'responsive') {
      setMode(underlyingMode);
      return;
    }
    const update = () => {
      setMode(splitOrUnifiedBasedOnWidth());
    };
    update();
    window.addEventListener('resize', update);
    return () => window.removeEventListener('resize', update);
  }, [underlyingMode, setMode]);

  return mode;
}

type ActiveCommentLine = {
  line: number;
  side: 'LEFT' | 'RIGHT';
} | null;

function ComparisonViewFile({
  diff,
  comparison,
  collapsed,
  setCollapsed,
  generatedStatus,
  displayMode,
  setRef,
}: {
  diff: ParsedDiff;
  comparison: Comparison;
  collapsed: boolean;
  setCollapsed: (isCollapsed: boolean) => void;
  generatedStatus: GeneratedStatus;
  displayMode: ComparisonDisplayMode;
  setRef?: (path: string, element: HTMLDivElement | null) => void;
}) {
  const path = diff.newFileName ?? diff.oldFileName ?? '';
  const reviewMode = useAtomValue(reviewModeAtom);

  // State for active comment input in review mode
  const [activeCommentLine, setActiveCommentLine] = useState<ActiveCommentLine>(null);
  // State for file-level comment input
  const [showFileComment, setShowFileComment] = useState(false);

  // Get pending comments for the current PR when in review mode
  const pendingComments = useAtomValue(
    pendingCommentsAtom(reviewMode.prNumber ?? ''),
  );

  // Filter pending comments for the current file
  const filePendingComments = useMemo(() => {
    return pendingComments.filter(comment => comment.path === path);
  }, [pendingComments, path]);

  // Use PR-aware key when in review mode to enable reset on PR updates
  const reviewKey = useMemo(() => {
    if (reviewMode.active && reviewMode.prNumber != null && reviewMode.prHeadHash != null) {
      return reviewedFileKeyForPR(Number(reviewMode.prNumber), reviewMode.prHeadHash, path);
    }
    return reviewedFileKey(comparison, path);
  }, [reviewMode.active, reviewMode.prNumber, reviewMode.prHeadHash, path, comparison]);

  const [reviewed, setReviewed] = useAtom(reviewedFilesAtom(reviewKey));

  // Reviewed files are always collapsed. To expand, uncheck the review first.
  const effectiveCollapsed = collapsed || reviewed;

  const handleToggleReviewed = useCallback(() => {
    setReviewed(prev => !prev);
  }, [setReviewed]);

  // Comment click handler - only active in review mode
  const onCommentClick = useCallback(
    (lineNumber: number, side: 'LEFT' | 'RIGHT', _path: string) => {
      if (reviewMode.active) {
        setActiveCommentLine({line: lineNumber, side});
      }
    },
    [reviewMode.active],
  );

  // File comment click handler - only active in review mode
  const onFileCommentClick = useCallback(
    (_path: string) => {
      if (reviewMode.active) {
        setShowFileComment(true);
      }
    },
    [reviewMode.active],
  );

  const context: Context = {
    id: {path, comparison},
    copy: platform.clipboardCopy,
    openFile: () => platform.openFile(path),
    // only offer clickable line numbers for comparisons against head, otherwise line numbers will be inaccurate
    openFileToLine: comparisonIsAgainstHead(comparison)
      ? (line: number) => platform.openFile(path, {line})
      : undefined,

    async fetchAdditionalLines(id, start, numLines) {
      serverAPI.postMessage({
        type: 'requestComparisonContextLines',
        numLines,
        start,
        id,
      });

      const result = await serverAPI.nextMessageMatching(
        'comparisonContextLines',
        msg => msg.path === id.path,
      );

      return result.lines;
    },
    // We must ensure the lineRange gets invalidated when the underlying file's context lines
    // have changed.
    // This depends on the comparison:
    // for Committed: the commit hash is included in the Comparison, thus the cached data will always be accurate.
    // for Uncommitted, Head, and Stack:
    // by referencing the latest head commit's hash, we ensure this selector reloads when the head commit changes.
    // These comparisons are all against the working copy (not exactly head),
    // but there's no change that could be made that would affect the context lines without
    // also changing the head commit's hash.
    // Note: we use latestHeadCommit WITHOUT previews, so we don't accidentally cache the file content
    // AGAIN on the same data while waiting for some new operation to finish.
    // eslint-disable-next-line react-hooks/rules-of-hooks
    useComparisonInvalidationKeyHook: () => useAtomValue(latestHeadCommit)?.hash ?? '',
    useThemeHook: () => useAtomValue(themeState),
    t,
    collapsed: effectiveCollapsed,
    setCollapsed,
    display: displayMode,
    reviewed,
    onToggleReviewed: handleToggleReviewed,
    // Wire up comment click handler when in review mode
    onCommentClick: reviewMode.active ? onCommentClick : undefined,
    // Wire up file comment click handler when in review mode
    onFileCommentClick: reviewMode.active ? onFileCommentClick : undefined,
  };
  return (
    <div
      className="comparison-view-file"
      key={path}
      ref={element => setRef?.(path, element)}>
      <ErrorBoundary>
        <SplitDiffView ctx={context} patch={diff} path={path} generatedStatus={generatedStatus} />
        {/* Inline comment input when a line is active */}
        {reviewMode.active && activeCommentLine != null && (
          <div className="inline-comment-input-container">
            <CommentInput
              prNumber={reviewMode.prNumber!}
              type="inline"
              path={path}
              line={activeCommentLine.line}
              side={activeCommentLine.side}
              onCancel={() => setActiveCommentLine(null)}
            />
          </div>
        )}
        {/* File-level comment input */}
        {reviewMode.active && showFileComment && (
          <div className="inline-comment-input-container">
            <CommentInput
              prNumber={reviewMode.prNumber!}
              type="file"
              path={path}
              onCancel={() => setShowFileComment(false)}
            />
          </div>
        )}
        {/* Display pending comments for this file */}
        {reviewMode.active && filePendingComments.length > 0 && (
          <div className="file-pending-comments">
            {filePendingComments.map(comment => (
              <PendingCommentDisplay
                key={comment.id}
                comment={comment}
                prNumber={reviewMode.prNumber!}
              />
            ))}
          </div>
        )}
      </ErrorBoundary>
    </div>
  );
}
