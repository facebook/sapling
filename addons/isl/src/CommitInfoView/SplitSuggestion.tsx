/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from '../Banner';
import {Column} from '../ComponentUtils';
import {Internal} from '../Internal';
import {Divider} from '../components/Divider';
import GatedComponent from '../components/GatedComponent';
import {T, t} from '../i18n';
import {localStorageBackedAtom} from '../jotaiUtils';
import {
  MAX_FILES_ALLOWED_FOR_DIFF_STAT,
  SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS,
} from '../sloc/diffStatConstants';
import {useFetchSignificantLinesOfCode} from '../sloc/useFetchSignificantLinesOfCode';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {type CommitInfo} from '../types';
import {useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';

export const splitSuggestionEnabled = localStorageBackedAtom<boolean>(
  'isl.split-suggestion-enabled',
  true,
);

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit);
  if (
    significantLinesOfCode == null ||
    significantLinesOfCode <= SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS
  ) {
    return null;
  }
  return (
    <>
      <Divider />
      <Banner
        tooltip={t('This commit has $sloc significant lines of code (top 10%)', {
          replace: {$sloc: String(significantLinesOfCode)},
        })}
        kind={BannerKind.default}
        icon={<Icon size="M" icon="lightbulb" color="blue" />}
        alwaysShowButtons
        buttons={<SplitButton trackerEventName="SplitOpenFromSplitSuggestion" commit={commit} />}>
        <Column alignStart style={{gap: 0}}>
          <b>
            <T>Consider splitting up this commit</T>
          </b>
          <T>Small Diffs lead to less SEVs & quicker review times</T>
        </Column>
      </Banner>
    </>
  );
}

export default function SplitSuggestion({commit}: {commit: CommitInfo}) {
  const enabled = useAtomValue(splitSuggestionEnabled);
  if (!enabled || commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT) {
    return null;
  }
  // using a gated component here to avoid exposing when diff size is too big  to show the split suggestion
  return (
    <GatedComponent featureFlag={Internal.featureFlags?.ShowSplitSuggestion}>
      <SplitSuggestionImpl commit={commit} />
    </GatedComponent>
  );
}
