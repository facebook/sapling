/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {FlexRow, ScrollY} from './ComponentUtils';
import {Modal} from './Modal';
import {StackEditConfirmButtons} from './StackEditConfirmButtons';
import {StackEditSubTree} from './StackEditSubTree';
import {loadingStackState, editingStackHashes} from './stackEditState';
import {useRecoilValue} from 'recoil';

/// Show a <Modal /> when editing a stack.
export function MaybeEditStackModal() {
  const loadingState = useRecoilValue(loadingStackState);
  const stackHashes = useRecoilValue(editingStackHashes);

  const isEditing = stackHashes.size > 0;
  const isLoaded = isEditing && loadingState.state === 'hasValue';

  return isLoaded ? <LoadedEditStackModal /> : null;
}

/// A <Modal /> for stack editing UI.
function LoadedEditStackModal() {
  return (
    <Modal>
      <ScrollY maxSize="70vh">
        <StackEditSubTree />
      </ScrollY>
      <FlexRow style={{padding: 'var(--pad) 0', justifyContent: 'flex-end'}}>
        <StackEditConfirmButtons />
      </FlexRow>
    </Modal>
  );
}
