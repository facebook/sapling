/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Row, FlexSpacer, ScrollY, Center} from '../../ComponentUtils';
import {ErrorNotice} from '../../ErrorNotice';
import {Modal} from '../../Modal';
import {tracker} from '../../analytics';
import {T, t} from '../../i18n';
import {SplitStackEditPanel, SplitStackToolbar} from './SplitStackEditPanel';
import {StackEditConfirmButtons} from './StackEditConfirmButtons';
import {StackEditSubTree} from './StackEditSubTree';
import {loadingStackState, editingStackIntentionHashes} from './stackEditState';
import {VSCodePanels, VSCodePanelTab, VSCodePanelView} from '@vscode/webview-ui-toolkit/react';
import {useAtom, useAtomValue} from 'jotai';
import {useState} from 'react';
import {Icon} from 'shared/Icon';

import './EditStackModal.css';

/// Show a <Modal /> when editing a stack.
export function MaybeEditStackModal() {
  const loadingState = useAtomValue(loadingStackState);
  const [[stackIntention, stackHashes], setStackIntention] = useAtom(editingStackIntentionHashes);

  const isEditing = stackHashes.size > 0;
  const isLoaded = isEditing && loadingState.state === 'hasValue';

  return isLoaded ? (
    stackIntention === 'split' ? (
      <LoadedSplitModal />
    ) : (
      <LoadedEditStackModal />
    )
  ) : isEditing ? (
    <Modal
      dataTestId="edit-stack-loading"
      dismiss={() => {
        // allow dismissing in loading state in case it gets stuck
        setStackIntention(['general', new Set()]);
      }}>
      <Center
        // add spacing to account for action buttons, so the modal is the same size during and after loading
        style={{paddingBottom: 'calc(24px + 2 * var(--pad))'}}
        className={stackIntention === 'split' ? 'interactive-split' : 'edit-stack-modal-panels'}>
        {loadingState.state === 'hasError' ? (
          <ErrorNotice error={new Error(loadingState.error)} title={t('Loading stack failed')} />
        ) : (
          <Row>
            <Icon icon="loading" size="M" />
            {(loadingState.state === 'loading' && loadingState.message) ?? null}
          </Row>
        )}
      </Center>
    </Modal>
  ) : null;
}

/** A Modal for dedicated split UI. Subset of `LoadedEditStackModal`. */
function LoadedSplitModal() {
  return (
    <Modal dataTestId="interactive-split-modal">
      <SplitStackEditPanel />
      <Row style={{padding: 'var(--pad) 0', justifyContent: 'flex-end'}}>
        <StackEditConfirmButtons />
      </Row>
    </Modal>
  );
}

/** A Modal for general stack editing UI. */
function LoadedEditStackModal() {
  type Tab = 'commits' | 'files' | 'split';
  const [activeTab, setActiveTab] = useState<Tab>('commits');
  const getPanelViewStyle = (tab: string): React.CSSProperties => {
    return {
      overflow: 'unset',
      display: 'block',
      padding: tab === activeTab ? 'var(--pad) 0 0 0' : '0',
    };
  };

  return (
    <Modal>
      <VSCodePanels
        className="edit-stack-modal-panels"
        activeid={`tab-${activeTab}`}
        style={{
          // Allow dropdown to show content.
          overflow: 'unset',
        }}
        onChange={e => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const tab: Tab | undefined = (e.target as any)?.activetab?.id?.replace('tab-', '');
          tab && setActiveTab(tab);
          tab && tracker.track('StackEditChangeTab', {extras: {tab}});
        }}>
        <VSCodePanelTab id="tab-commits">
          <T>Commits</T>
        </VSCodePanelTab>
        {/* TODO: reenable the "files" tab */}
        {/* <VSCodePanelTab id="tab-files">
          <T>Files</T>
        </VSCodePanelTab> */}
        <VSCodePanelTab id="tab-split">
          <T>Split</T>
        </VSCodePanelTab>
        <VSCodePanelView style={getPanelViewStyle('commits')} id="view-commits">
          {/* Skip rendering (which might trigger slow dependency calculation) if the tab is inactive */}
          <ScrollY maxSize="calc((100vh / var(--zoom)) - 200px)">
            {activeTab === 'commits' && (
              <StackEditSubTree
                activateSplitTab={() => {
                  setActiveTab('split');
                  tracker.track('StackEditInlineSplitButton');
                }}
              />
            )}
          </ScrollY>
        </VSCodePanelView>
        {/* TODO: reenable the "files" tab */}
        {/* <VSCodePanelView style={getPanelViewStyle('files')} id="view-files">
          {activeTab === 'files' && <FileStackEditPanel />}
        </VSCodePanelView> */}
        <VSCodePanelView style={getPanelViewStyle('split')} id="view-split">
          {activeTab === 'split' && <SplitStackEditPanel />}
        </VSCodePanelView>
      </VSCodePanels>
      <Row style={{padding: 'var(--pad) 0', justifyContent: 'flex-end'}}>
        {activeTab === 'split' && <SplitStackToolbar />}
        <FlexSpacer />
        <StackEditConfirmButtons />
      </Row>
    </Modal>
  );
}
