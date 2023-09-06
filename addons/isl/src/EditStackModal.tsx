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
import {T} from './i18n';
import {loadingStackState, editingStackHashes} from './stackEditState';
import {VSCodePanels, VSCodePanelTab, VSCodePanelView} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
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
  type Tab = 'commits' | 'files';
  const [activeTab, setActiveTab] = useState<Tab>('commits');
  return (
    <Modal>
      <VSCodePanels
        onChange={e => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const tab: Tab | undefined = (e.target as any)?.activetab?.id?.replace('tab-', '');
          tab && setActiveTab(tab);
        }}>
        <VSCodePanelTab id="tab-commits">
          <T>Commits</T>
        </VSCodePanelTab>
        <VSCodePanelTab id="tab-files">
          <T>Files</T>
        </VSCodePanelTab>
        <VSCodePanelView id="view-commits">
          {/* Skip rendering (which might trigger slow dependency calculation) if the tab is inactive */}
          <ScrollY maxSize="70vh">{activeTab === 'commits' && <StackEditSubTree />}</ScrollY>
        </VSCodePanelView>
        <VSCodePanelView id="view-files"></VSCodePanelView>
      </VSCodePanels>
      <FlexRow style={{padding: 'var(--pad) 0', justifyContent: 'flex-end'}}>
        <StackEditConfirmButtons />
      </FlexRow>
    </Modal>
  );
}
