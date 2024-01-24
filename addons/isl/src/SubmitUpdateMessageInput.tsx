/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from './types';

import {diffUpdateMessagesState} from './CommitInfoView/CommitInfoState';
import {MinHeightTextField} from './CommitInfoView/MinHeightTextField';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T} from './i18n';
import {useRef} from 'react';
import {useRecoilState, useRecoilValue} from 'recoil';

export function SubmitUpdateMessageInput({commits}: {commits: Array<CommitInfo>}) {
  const provider = useRecoilValue(codeReviewProvider);
  const ref = useRef(null);

  // typically only one commit, but if you've selected multiple, we key the message on all hashes together.
  const key = commits.map(c => c.hash).join(',');
  const [message, setMessage] = useRecoilState(diffUpdateMessagesState(key));
  if (message == null || provider?.supportsUpdateMessage !== true) {
    return null;
  }
  return (
    <MinHeightTextField
      ref={ref}
      keepNewlines
      style={{width: '100%'}}
      value={message}
      onInput={(e: KeyboardEvent) => setMessage((e.target as HTMLInputElement).value)}>
      <T>Update Message</T>
    </MinHeightTextField>
  );
}
