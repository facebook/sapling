/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';

import {Badge} from '../components/Badge';
import {T} from '../i18n';
import {useFetchSignificantLinesOfCode} from '../sloc/useFetchSignificantLinesOfCode';

type Props = {commit: CommitInfo};

export default function SlocBadge({commit}: Props) {
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
