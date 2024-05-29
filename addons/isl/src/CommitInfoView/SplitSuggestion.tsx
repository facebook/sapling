/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Banner, BannerKind} from '../Banner';
import {Internal} from '../Internal';
import {Tooltip} from '../Tooltip';
import {Divider} from '../components/Divider';
import {useFeatureFlagSync} from '../featureFlags';
import {T} from '../i18n';
import {useFetchSignificantLinesOfCode} from '../sloc/useFetchSignificantLinesOfCode';
import {SplitButton} from '../stackEdit/ui/SplitButton';
import {type CommitInfo} from '../types';
import {Icon} from 'shared/Icon';

function SplitSuggestionImpl({commit}: {commit: CommitInfo}) {
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit);
  if (significantLinesOfCode <= 100) {
    return null;
  }
  return (
    <>
      <Divider />
      <Banner
        tooltip=""
        kind={BannerKind.green}
        icon={<Icon icon="info" />}
        alwaysShowButtons
        buttons={
          <SplitButton
            style={{
              border: '1px solid var(--button-secondary-hover-background)',
            }}
            trackerEventName="SplitOpenFromSplitSuggestion"
            commit={commit}
          />
        }>
        <div>
          <T>Pro tip: Small Diffs lead to less SEVs, quicker review times and happier teams.</T>
          &nbsp;
          <Tooltip
            inline={true}
            trigger="hover"
            title={`Significant Lines of Code (SLOC): ${significantLinesOfCode}, this puts your diff in the top 10% of diffs. `}>
            <T>This diff is a bit big</T>
          </Tooltip>
          <T>, consider splitting it up</T>
        </div>
      </Banner>
    </>
  );
}

function GatedSplitSuggestion({commit}: {commit: CommitInfo}) {
  const showSplitSuggestion = useFeatureFlagSync(Internal.featureFlags?.ShowSplitSuggestion);

  if (!showSplitSuggestion) {
    return null;
  }
  return <SplitSuggestionImpl commit={commit} />;
}

export default function SplitSuggestion({commit}: {commit: CommitInfo}) {
  if (commit.totalFileCount > 25) {
    return null;
  }
  // using a gated component to avoid exposing when diff size is too big  to show the split suggestion
  return <GatedSplitSuggestion commit={commit} />;
}
