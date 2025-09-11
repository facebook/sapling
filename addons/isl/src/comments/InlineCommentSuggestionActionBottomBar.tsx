/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Row} from 'isl-components/Flex';
import {Icon} from 'isl-components/Icon';

const styles = stylex.create({
  actionBarRow: {
    alignItems: 'flex-start',
    marginBlock: 1, // Ensure buttons in different themes have sufficient height, regardless of their border
  },
});

export default function InlineCommentActionBottomBar({
  resolved = false,
  onAccept,
  onReject,
  acceptLabel,
  rejectLabel,
  isToggle = false,
}: {
  resolved: boolean;
  onAccept: () => unknown;
  onReject: () => unknown;
  acceptLabel?: string;
  rejectLabel?: string;
  isToggle?: boolean;
}) {
  return isToggle ? (
    <Row xstyle={styles.actionBarRow}>
      {!resolved ? (
        <Button onClick={onAccept} primary={true}>
          {acceptLabel ? acceptLabel : 'Apply'}
          <Icon icon="check" />
        </Button>
      ) : (
        <Button onClick={onReject}>
          {rejectLabel ? rejectLabel : 'Discard'}
          <Icon icon="close" />
        </Button>
      )}
    </Row>
  ) : (
    <Row xstyle={styles.actionBarRow}>
      <Button onClick={onAccept} primary={true}>
        {acceptLabel ? acceptLabel : 'Apply'}
        <Icon icon="check" />
      </Button>
      <Button onClick={onReject}>
        {rejectLabel ? rejectLabel : 'Discard'}
        <Icon icon="close" />
      </Button>
    </Row>
  );
}
