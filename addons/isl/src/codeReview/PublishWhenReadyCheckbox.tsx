/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Checkbox} from 'isl-components/Checkbox';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {publishWhenReady} from '../atoms/submitOptionAtoms';
import {t, T} from '../i18n';
import {codeReviewProvider} from './CodeReviewInfo';

export {publishWhenReady} from '../atoms/submitOptionAtoms';

/**
 * Checkbox component for the "Publish when ready" option.
 * When enabled, diffs are automatically published after all CI signals pass.
 * Only shown for Phabricator repositories (GitHub doesn't support this feature).
 *
 * The bidirectional relationship with SubmitAsDraftCheckbox is enforced at the atom level:
 * - Checking "Publish when ready" automatically enables "Submit as Draft"
 * - Unchecking "Submit as Draft" automatically disables "Publish when ready"
 * See atoms/submitOptionAtoms.ts for implementation details.
 */
export function PublishWhenReadyCheckbox() {
  const [isPublishWhenReady, setPublishWhenReady] = useAtom(publishWhenReady);
  const provider = useAtomValue(codeReviewProvider);

  // Only show for Phabricator, not GitHub
  if (provider?.name !== 'phabricator') {
    return null;
  }

  return (
    <Checkbox checked={isPublishWhenReady} onChange={setPublishWhenReady}>
      <Tooltip title={t('publishWhenReadyTooltip')}>
        <T>Publish when ready</T>
      </Tooltip>
    </Checkbox>
  );
}
