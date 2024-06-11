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
import {uncommittedChangesWithPreviews} from '../previews';
import {
  MAX_FILES_ALLOWED_FOR_DIFF_STAT,
  SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS,
} from '../sloc/diffStatConstants';
import {
  useFetchPendingSignificantLinesOfCode,
  useFetchSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {type CommitInfo} from '../types';
import {commitMode} from './CommitInfoState';
import {useAtomValue} from 'jotai';
import {Suspense} from 'react';
import {Icon} from 'shared/Icon';

export const splitSuggestionEnabled = localStorageBackedAtom<boolean>(
  'isl.split-suggestion-enabled',
  true,
);

function SuggestionBanner({
  tooltip,
  buttons,
  children,
}: {
  tooltip: string;
  buttons?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <>
      <Divider />
      <Banner
        tooltip={tooltip}
        kind={BannerKind.default}
        icon={<Icon size="M" icon="lightbulb" color="blue" />}
        alwaysShowButtons
        buttons={buttons}>
        <Column alignStart style={{gap: 0}}>
          {children}
        </Column>
      </Banner>
    </>
  );
}

function NewCommitSuggestion() {
  const pendingSignificantLinesOfCode = useFetchPendingSignificantLinesOfCode();
  if (pendingSignificantLinesOfCode == null) {
    return null;
  }

  if (pendingSignificantLinesOfCode > SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS) {
    return (
      <SuggestionBanner
        tooltip={t('This commit would have $sloc significant lines of code (top 10%)', {
          replace: {$sloc: String(pendingSignificantLinesOfCode)},
        })}>
        <b>
          <T>Consider unselecting some of these changes</T>
        </b>
        <T>Small Diffs lead to less SEVs & quicker review times</T>
      </SuggestionBanner>
    );
  }
}

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const mode = useAtomValue(commitMode);
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit) ?? -1;
  const uncommittedChanges = useAtomValue(uncommittedChangesWithPreviews);

  // no matter what if the commit is over the threshold, we show the split suggestion
  if (significantLinesOfCode > SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS) {
    return (
      <SuggestionBanner
        tooltip={t('This commit has $sloc significant lines of code (top 10%)', {
          replace: {$sloc: String(significantLinesOfCode)},
        })}
        buttons={<SplitButton trackerEventName="SplitOpenFromSplitSuggestion" commit={commit} />}>
        <b>
          <T>Consider splitting up this commit</T>
        </b>
        <T>Small Diffs lead to less SEVs & quicker review times</T>
      </SuggestionBanner>
    );
  }

  // if there are uncommitted changes, let's (maybe) show the suggestion to make a new commit
  if (uncommittedChanges.length > 0 && mode === 'commit') {
    return <NewCommitSuggestion />;
  }

  // no need to show any suggestion
  return null;
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
