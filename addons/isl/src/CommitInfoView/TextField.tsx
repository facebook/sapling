/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useRef, useEffect} from 'react';

export function CommitInfoTextField({
  name,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      const inner = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
      inner?.focus();
    }
  }, [autoFocus, ref]);

  const onInput = (event: {target: EventTarget | null}) => {
    setEditedCommitMessage((event?.target as HTMLInputElement)?.value);
  };

  const fieldKey = name.toLowerCase().replace(/\s/g, '-');

  return (
    <div className="commit-info-field">
      <VSCodeTextField
        ref={ref}
        value={editedMessage}
        data-testid={`commit-info-${fieldKey}-field`}
        onInput={onInput}
      />
    </div>
  );
}
