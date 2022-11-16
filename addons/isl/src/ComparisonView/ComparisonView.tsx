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
import {T, t} from '../i18n';
import {latestHeadCommit} from '../serverAPIState';
import {themeState} from '../theme';
import {currentComparisonMode} from './atoms';
import {ThemeProvider, BaseStyles} from '@primer/react';
import {VSCodeButton, VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {parsePatch} from 'diff';
import {useEffect} from 'react';
import {atomFamily, selectorFamily, useRecoilValue, useSetRecoilState} from 'recoil';
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

const currentComparisonData = atomFamily<Result<Array<ParsedDiff>> | null, Comparison>({
  key: 'currentComparisonData',
  default: (_comparison: Comparison) => null,
  effects: (comparison: Comparison) => [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('comparison', event => {
        if (comparison.type === event.comparison.type) {
          setSelf(mapResult(event.data.diff, parsePatchAndFilter));
        }
      });
      return () => disposable.dispose();
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

export default function ComparisonView({comparison}: {comparison: Comparison}) {
  useEffect(() => {
    serverAPI.postMessage({type: 'requestComparison', comparison});
  }, [comparison]);
  const theme = useRecoilValue(themeState);

  const compared = useRecoilValue(currentComparisonData(comparison));

  return (
    <div data-testid="comparison-view" className="comparison-view">
      <ThemeProvider colorMode={theme === 'light' ? 'day' : 'night'}>
        <SplitDiffViewPrimerStyles />
        <BaseStyles className="comparison-view-base-styles">
          <ComparisonViewHeader comparison={comparison} />
          <div className="comparison-view-details">
            {compared == null ? (
              <Icon icon="loading" />
            ) : compared.error != null ? (
              <ErrorNotice error={compared.error} title={t('Unable to load comparison')} />
            ) : compared.value.length === 0 ? (
              <EmptyState>
                <T>No Changes</T>
              </EmptyState>
            ) : (
              compared.value.map((parsed, i) => (
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

  return (
    <>
      <div className="comparison-view-header">
        <VSCodeDropdown
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
