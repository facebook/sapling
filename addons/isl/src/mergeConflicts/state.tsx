/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Internal} from '../Internal';
import {T, t} from '../i18n';
import {localStorageBackedAtom} from '../jotaiUtils';
import {Checkbox} from 'isl-components/Checkbox';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';

export const shouldAutoResolveAllBeforeContinue = localStorageBackedAtom<boolean>(
  'isl.auto-resolve-before-continue',
  // OSS doesn't typically use merge drivers, so `sl resolve --all` would be added overhead for little gain.
  // You can still configure this in settings if you want.
  Internal.autoRunMergeDriversByDefault === true,
);

export function AutoResolveSettingCheckbox({subtle}: {subtle?: boolean}) {
  const [shouldAutoResolve, setShouldAutoResolve] = useAtom(shouldAutoResolveAllBeforeContinue);

  const label = <T>Auto-run Merge Drivers</T>;
  return (
    <Tooltip
      title={t(
        'Whether to run `sl resolve --all` before `sl continue`. ' +
          'This runs automated merge drivers to regenerate generated files.\n' +
          'This is usually needed to finish a merge, but merge drivers can be slow.',
      )}>
      <Checkbox checked={shouldAutoResolve} onChange={setShouldAutoResolve}>
        {subtle ? <Subtle>{label}</Subtle> : label}
      </Checkbox>
    </Tooltip>
  );
}

export const CONFLICT_SIDE_LABELS = {
  /* Label used for the local / destination side of a conflict */
  local: t('dest - rebasing onto'),
  /* Shortened label used for the local / destination side of a conflict, when there's not good space for the full label */
  localShort: t('dest'),
  /* Label used for the incoming / source side of a conflict */
  incoming: t('source - being rebased'),
  /* Shortened label used for the incoming / source side of a conflict, when there's not good space for the full label */
  incomingShort: t('source'),
};
