/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result} from '../types';
import type {LineRangeParams} from './SplitDiffView/types';
import type {Comparison} from 'shared/Comparison';
import type {ParsedDiff} from 'shared/patch/parse';

import serverAPI from '../ClientToServerAPI';
import {EmptyState} from '../EmptyState';
import {ErrorBoundary, ErrorNotice} from '../ErrorNotice';
import {Tooltip} from '../Tooltip';
import {T, t} from '../i18n';
import platform from '../platform';
import {latestHeadCommit} from '../serverAPIState';
import {SplitDiffView} from './SplitDiffView';
import {currentComparisonMode} from './atoms';
import {VSCodeButton, VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {useCallback, useEffect, useState} from 'react';
import {atomFamily, selectorFamily, useRecoilState, useSetRecoilState} from 'recoil';
import {comparisonIsAgainstHead, labelForComparison, ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';
import {parsePatch} from 'shared/patch/parse';

import './ComparisonView.css';

/**
 * Transform Result<T> to Result<U> by applying `fn` on result.value.
 * If the result is an error, just return it unchanged.
 */
function mapResult<T, U>(result: Result<T>, fn: (t: T) => U): Result<U> {
  return result.error == null ? {value: fn(result.value)} : result;
}

function parsePatchAndFilter(patch: string): ReturnType<typeof parsePatch> {
  const result = parsePatch(patch);
  return result.filter(
    // empty patches and other weird situations can cause invalid files to get parsed, ignore these entirely
    diff => diff.hunks.length > 0 || diff.newFileName != null || diff.oldFileName != null,
  );
}

const currentComparisonData = atomFamily<
  {isLoading: boolean; data: Result<Array<ParsedDiff>> | null},
  Comparison
>({
  key: 'currentComparisonData',
  default: (_comparison: Comparison) => ({isLoading: true, data: null}),
  effects: (comparison: Comparison) => [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('comparison', event => {
        if (comparison.type === event.comparison.type) {
          setSelf({isLoading: false, data: mapResult(event.data.diff, parsePatchAndFilter)});
        }
      });
      return () => disposable.dispose();
    },
    // You can trigger a refresh just by setting isLoading: true
    ({onSet}) => {
      onSet(value => {
        if (value.isLoading) {
          serverAPI.postMessage({type: 'requestComparison', comparison});
        }
      });
    },
  ],
});

export const lineRange = selectorFamily<
  string[],
  LineRangeParams<{path: string; comparison: Comparison}>
>({
  key: 'lineRange',
  get:
    params =>
    ({get}) => {
      // We must ensure this lineRange gets invalidated when the underlying file's context lines
      // have changed.
      // This depends on the comparison:
      // for Committed: the commit hash is included in the Comparison, thus the cached data will always be accurate.
      // for Uncommitted, Head, and Stack:
      // by referencing the latest head commit atom, we ensure this selector reloads when the head commit changes.
      // These comparisons are all against the working copy (not exactly head),
      // but there's no change that could be made that would affect the context lines without
      // also changing the head commit's hash.
      // Note: we use latestHeadCommit WITHOUT previews, so we don't accidentally cache the file content
      // AGAIN on the same data while waiting for some new operation to finish.
      get(latestHeadCommit);

      serverAPI.postMessage({type: 'requestComparisonContextLines', ...params});

      return new Promise(res => {
        const disposable = serverAPI.onMessageOfType('comparisonContextLines', event => {
          res(event.lines);
          disposable.dispose();
        });
      });
    },
});

function useComparisonData(comparison: Comparison) {
  const [compared, setCompared] = useRecoilState(currentComparisonData(comparison));
  const reloadComparison = useCallback(() => {
    // setting comparisonData's isLoading: true triggers a fetch
    setCompared(data => ({...data, isLoading: true}));
  }, [setCompared]);
  return [compared, reloadComparison] as const;
}

export default function ComparisonView({comparison}: {comparison: Comparison}) {
  const [compared, reloadComparison] = useComparisonData(comparison);

  // any time the comparison changes, fetch the diff
  useEffect(reloadComparison, [comparison, reloadComparison]);

  const [collapsedFiles, setCollapsedFile] = useCollapsedFilesState(compared);

  return (
    <div data-testid="comparison-view" className="comparison-view">
      <ComparisonViewHeader comparison={comparison} />
      <div className="comparison-view-details">
        {compared.data == null ? (
          <Icon icon="loading" />
        ) : compared.data.error != null ? (
          <ErrorNotice error={compared.data.error} title={t('Unable to load comparison')} />
        ) : compared.data.value.length === 0 ? (
          <EmptyState>
            <T>No Changes</T>
          </EmptyState>
        ) : (
          compared.data.value.map((parsed, i) => (
            <ComparisonViewFile
              diff={parsed}
              comparison={comparison}
              key={i}
              collapsed={collapsedFiles.get(parsed.newFileName ?? '') ?? false}
              setCollapsed={(collapsed: boolean) =>
                setCollapsedFile(parsed.newFileName ?? '', collapsed)
              }
            />
          ))
        )}
      </div>
    </div>
  );
}

const defaultComparisons = [
  ComparisonType.UncommittedChanges as const,
  ComparisonType.HeadChanges as const,
  ComparisonType.StackChanges as const,
];
function ComparisonViewHeader({comparison}: {comparison: Comparison}) {
  const setComparisonMode = useSetRecoilState(currentComparisonMode);
  const [compared, reloadComparison] = useComparisonData(comparison);

  return (
    <>
      <div className="comparison-view-header">
        <span className="comparison-view-header-group">
          <VSCodeDropdown
            data-testid="comparison-view-picker"
            value={comparison.type}
            onChange={event =>
              setComparisonMode(previous => ({
                ...previous,
                comparison: {
                  type: (event as React.FormEvent<HTMLSelectElement>).currentTarget
                    .value as (typeof defaultComparisons)[0],
                },
              }))
            }>
            {defaultComparisons.map(comparison => (
              <VSCodeOption value={comparison} key={comparison}>
                <T>{labelForComparison({type: comparison})}</T>
              </VSCodeOption>
            ))}
            {!defaultComparisons.includes(comparison.type as (typeof defaultComparisons)[0]) ? (
              <VSCodeOption value={comparison.type} key={comparison.type}>
                <T>{labelForComparison(comparison)}</T>
              </VSCodeOption>
            ) : null}
          </VSCodeDropdown>
          <Tooltip
            delayMs={1000}
            title={t('Reload this comparison. Comparisons do not refresh automatically.')}>
            <VSCodeButton appearance="secondary" onClick={reloadComparison}>
              <Icon icon="refresh" data-testid="comparison-refresh-button" />
            </VSCodeButton>
          </Tooltip>
          {compared.isLoading ? <Icon icon="loading" data-testid="comparison-loading" /> : null}
        </span>
        <VSCodeButton
          data-testid="close-comparison-view-button"
          appearance="icon"
          onClick={() => setComparisonMode(previous => ({...previous, visible: false}))}>
          <Icon icon="x" />
        </VSCodeButton>
      </div>
    </>
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

function ComparisonViewFile({
  diff,
  comparison,
  collapsed,
  setCollapsed,
}: {
  diff: ParsedDiff;
  comparison: Comparison;
  collapsed: boolean;
  setCollapsed: (isCollapsed: boolean) => void;
}) {
  const path = diff.newFileName ?? diff.oldFileName ?? '';
  const context = {
    id: {path, comparison},
    atoms: {lineRange},
    translate: t,
    copy: platform.clipboardCopy,
    openFile: () => platform.openFile(path),
    // only offer clickable line numbers for comparisons against head, otherwise line numbers will be inaccurate
    openFileToLine: comparisonIsAgainstHead(comparison)
      ? (line: number) => platform.openFile(path, {line})
      : undefined,
    collapsed,
    setCollapsed,
  };
  return (
    <div className="comparison-view-file" key={path}>
      <ErrorBoundary>
        <SplitDiffView ctx={context} patch={diff} path={path} />
      </ErrorBoundary>
    </div>
  );
}
