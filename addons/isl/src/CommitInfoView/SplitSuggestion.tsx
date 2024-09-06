/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Column} from '../ComponentUtils';
import {Internal} from '../Internal';
import {tracker} from '../analytics';
import {codeReviewProvider, diffSummary} from '../codeReview/CodeReviewInfo';
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
  useFetchSignificantLinesOfCode,
  useFetchPendingSignificantLinesOfCode,
  useFetchPendingAmendSignificantLinesOfCode,
} from '../sloc/useFetchSignificantLinesOfCode';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {type CommitInfo} from '../types';
import {commitMode} from './CommitInfoState';
import {Banner, BannerKind} from 'isl-components/Banner';
import {Button} from 'isl-components/Button';
import {ButtonGroup} from 'isl-components/ButtonGroup';
import {Divider} from 'isl-components/Divider';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';

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
          <ButtonGroup>
            {buttons}
            <DismissSuggestionButton />
          </ButtonGroup>
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
  const {slocInfo} = useFetchPendingSignificantLinesOfCode() ?? {};
  const pendingSignificantLinesOfCode = slocInfo?.strictSloc;

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
        <T>Small Diffs lead to quicker review times</T>
      </SuggestionBanner>
    );
  }
}

function AmendSuggestion() {
  const {slocInfo} = useFetchPendingAmendSignificantLinesOfCode();
  const pendingAmendSignificantLinesOfCode = slocInfo?.strictSloc;

  if (pendingAmendSignificantLinesOfCode == null) {
    return null;
  }
  if (pendingAmendSignificantLinesOfCode > SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS) {
    return (
      <SuggestionBanner
        tooltip={t(
          'Amending these changes would put the commit at $sloc significant lines of code (top 10%)',
          {replace: {$sloc: String(pendingAmendSignificantLinesOfCode)}},
        )}>
        <b>
          <T>Consider unselecting some of these changes</T>
        </b>
        <T>Small Diffs lead to quicker review times</T>
      </SuggestionBanner>
    );
  }
}

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const mode = useAtomValue(commitMode);
  const {slocInfo} = useFetchSignificantLinesOfCode(commit);
  const significantLinesOfCode = slocInfo?.strictSloc ?? -1;
  const uncommittedChanges = useAtomValue(uncommittedChangesWithPreviews);

  // no matter what if the commit is over the threshold, we show the split suggestion
  if (
    uncommittedChanges.length === 0 &&
    significantLinesOfCode > SLOC_THRESHOLD_FOR_SPLIT_SUGGESTIONS
  ) {
    return (
      <SuggestionBanner
        tooltip={t('This commit has $sloc significant lines of code (top 10%)', {
          replace: {$sloc: String(significantLinesOfCode)},
        })}
        buttons={<SplitButton trackerEventName="SplitOpenFromSplitSuggestion" commit={commit} />}>
        <b>
          <T>Consider splitting up this commit</T>
        </b>
        <T>Small Diffs lead to quicker review times</T>
      </SuggestionBanner>
    );
  }
  // if there are no uncommitted changes, we don't show the suggestion
  if (uncommittedChanges.length === 0) {
    return null;
  }

  // if there are uncommitted changes, let's (maybe) show the suggestion to make a new commit
  if (mode === 'commit') {
    return <NewCommitSuggestion />;
  } else {
    return <AmendSuggestion />;
  }
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
