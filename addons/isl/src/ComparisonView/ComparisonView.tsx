/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Result} from '../types';
import type {ParsedDiff} from 'diff';
import type {Comparison} from 'shared/Comparison';
import type {LineRangeParams} from 'shared/SplitDiffView/types';

import serverAPI from '../ClientToServerAPI';
import {EmptyState} from '../EmptyState';
import {ErrorNotice} from '../ErrorNotice';
import {Icon} from '../Icon';
import {Tooltip} from '../Tooltip';
import {T, t} from '../i18n';
import {latestHeadCommit} from '../serverAPIState';
import {themeState} from '../theme';
import {currentComparisonMode} from './atoms';
import {ThemeProvider, BaseStyles} from '@primer/react';
import {VSCodeButton, VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {parsePatch} from 'diff';
import {useCallback, useEffect} from 'react';
import {
  atomFamily,
  selectorFamily,
  useRecoilState,
  useRecoilValue,
  useSetRecoilState,
} from 'recoil';
import {labelForComparison, ComparisonType} from 'shared/Comparison';
import {SplitDiffView} from 'shared/SplitDiffView';
import SplitDiffViewPrimerStyles from 'shared/SplitDiffView/PrimerStyles';

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

  const theme = useRecoilValue(themeState);

  return (
    <div data-testid="comparison-view" className="comparison-view">
      <ThemeProvider colorMode={theme === 'light' ? 'day' : 'night'}>
        <SplitDiffViewPrimerStyles />
        <BaseStyles className="comparison-view-base-styles">
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
                <ComparisonViewFile diff={parsed} comparison={comparison} key={i} />
              ))
            )}
          </div>
        </BaseStyles>
      </ThemeProvider>
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
                    .value as typeof defaultComparisons[0],
                },
              }))
            }>
            {defaultComparisons.map(comparison => (
              <VSCodeOption value={comparison} key={comparison}>
                <T>{labelForComparison({type: comparison})}</T>
              </VSCodeOption>
            ))}
            {!defaultComparisons.includes(comparison.type as typeof defaultComparisons[0]) ? (
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

function ComparisonViewFile({diff, comparison}: {diff: ParsedDiff; comparison: Comparison}) {
  const path = diff.newFileName ?? diff.oldFileName ?? '';
  const context = {id: {path, comparison}, atoms: {lineRange}, translate: t};
  return (
    <div className="comparison-view-file" key={path}>
      <SplitDiffView ctx={context} patch={diff} path={path} />
    </div>
  );
}
