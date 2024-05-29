/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import {Internal} from '../Internal';
import {Badge} from '../components/Badge';
import {useFeatureFlagSync} from '../featureFlags';
import {T} from '../i18n';
import {useFetchSignificantLinesOfCode} from '../sloc/useFetchSignificantLinesOfCode';

type Props = {commit: CommitInfo};

function SlocBadge({commit}: Props) {
  const significantLinesOfCode = useFetchSignificantLinesOfCode(commit);
  if (significantLinesOfCode == null) {
    return null;
  }
  return (
    <>
      <T>SLOC</T>
      <Badge>{significantLinesOfCode}</Badge>
    </>
  );
}

export default function GatedSlocBadge({commit}: Props) {
  const showSplitSuggestion = useFeatureFlagSync(Internal.featureFlags?.ShowSplitSuggestion);

  if (!showSplitSuggestion) {
    return null;
  }
  return <SlocBadge commit={commit} />;
}
