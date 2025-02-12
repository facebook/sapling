/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import * as stylex from '@stylexjs/stylex';
import {useAtom, useAtomValue} from 'jotai';
import {useRef} from 'react';
import {diffUpdateMessagesState} from './CommitInfoView/CommitInfoState';
import {MinHeightTextField} from './CommitInfoView/MinHeightTextField';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T} from './i18n';

const styles = stylex.create({
  full: {
    width: '100%',
  },
});

export function multiSubmitUpdateMessage(commits: Array<CommitInfo>) {
  // Combine hashes to key the typed update message.
  // This is kind of volatile, since if you change your selection at all, the message will be cleared.
  // Note: order must be deterministic so that your selection order doesn't affect this key
  const orderedCommits = commits.map(c => c.hash);
  orderedCommits.sort();
  const key = orderedCommits.join(',');
  return diffUpdateMessagesState(key);
}

export function SubmitUpdateMessageInput({commits}: {commits: Array<CommitInfo>}) {
  const provider = useAtomValue(codeReviewProvider);
  const ref = useRef(null);

  // typically only one commit, but if you've selected multiple, we key the message on all hashes together.
  const [message, setMessage] = useAtom(multiSubmitUpdateMessage(commits));
  if (message == null || provider?.supportsUpdateMessage !== true) {
    return null;
  }
  return (
    <MinHeightTextField
      ref={ref}
      keepNewlines
      containerXstyle={styles.full}
      value={message}
      onInput={e => setMessage(e.currentTarget.value)}>
      <T>Update Message</T>
    </MinHeightTextField>
  );
}
