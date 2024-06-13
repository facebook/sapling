/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind, BannerTooltip} from '../Banner';
import {Column} from '../ComponentUtils';
import {Internal} from '../Internal';
import {Tooltip} from '../Tooltip';
import {tracker} from '../analytics';
import {codeReviewProvider, diffSummary} from '../codeReview/CodeReviewInfo';
import {Button} from '../components/Button';
import {Divider} from '../components/Divider';
import GatedComponent from '../components/GatedComponent';
import {T, t} from '../i18n';
import {localStorageBackedAtom} from '../jotaiUtils';
import platform from '../platform';
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
import {useAtom, useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';

export const splitSuggestionEnabled = localStorageBackedAtom<boolean>(
  'isl.split-suggestion-enabled',
  true,
);
const SEVEN_DAYS = 7 * 24 * 60 * 60 * 1000;
const dismissedAtom = localStorageBackedAtom<number | null>(`isl.dismissed-split-suggestion`, null);

function useDismissed() {
  const [dismissed, setDismissed] = useAtom(dismissedAtom);
  const isDismissed = () =>
    dismissed != null && new Date(dismissed) > new Date(Date.now() - SEVEN_DAYS);

  return {isDismissed, setDismissed};
}

function DismissSuggestionButton() {
  const {setDismissed} = useDismissed();
  return (
    <Tooltip title={t('Dismiss this suggestion for 7 days')}>
      <Button
        onClick={async () => {
          const ok = await platform.confirm(t('Dismiss this suggestion for 7 days?'));
          if (ok) {
            tracker.track('SplitSuggestionsDismissedForSevenDays');
            setDismissed(Date.now());
          }
        }}>
        <Icon icon="close" />
      </Button>
    </Tooltip>
  );
}
function SuggestionBanner({
  tooltip,
  buttons,
  children,
}: {
  tooltip: string;
  buttons?: React.ReactNode;
  children: React.ReactNode;
}) {
  const {isDismissed} = useDismissed();
  if (isDismissed()) {
    return null;
  }
  return (
    <>
      <Divider />
      <Banner
        kind={BannerKind.default}
        icon={<Icon size="M" icon="lightbulb" color="blue" />}
        alwaysShowButtons
        buttons={
          <>
            {buttons}
            <DismissSuggestionButton />
          </>
        }>
        <Tooltip title={tooltip}>
          <Column alignStart style={{gap: 0}}>
            {children}
          </Column>
        </Tooltip>
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

  const provider = useAtomValue(codeReviewProvider);
  const diffInfoResult = useAtomValue(diffSummary(commit.diffId));
  if (commit.diffId != null) {
    if (diffInfoResult.error || diffInfoResult?.value == null) {
      // don't show the suggestion until the diff is loaded to be sure it's not closed.
      return null;
    }
    const info = diffInfoResult.value;
    if (provider?.isDiffClosed(info)) {
      return null;
    }
  }

  if (
    !enabled ||
    commit.totalFileCount > MAX_FILES_ALLOWED_FOR_DIFF_STAT ||
    commit.phase === 'public'
  ) {
    return null;
  }
  // using a gated component here to avoid exposing when diff size is too big  to show the split suggestion
  return (
    <GatedComponent featureFlag={Internal.featureFlags?.ShowSplitSuggestion}>
      <SplitSuggestionImpl commit={commit} />
    </GatedComponent>
  );
}
