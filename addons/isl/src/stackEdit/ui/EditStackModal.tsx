/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Panels} from 'isl-components/Panels';
import {useAtom, useAtomValue} from 'jotai';
import {useState} from 'react';
import {Center, FlexSpacer, Row, ScrollY} from '../../ComponentUtils';
import {Modal} from '../../Modal';
import {tracker} from '../../analytics';
import {t} from '../../i18n';
import {AbsorbStackEditPanel} from './AbsorbStackEditPanel';
import {SplitStackEditPanel, SplitStackToolbar} from './SplitStackEditPanel';
import {StackEditConfirmButtons} from './StackEditConfirmButtons';
import {StackEditSubTree} from './StackEditSubTree';
import {editingStackIntentionHashes, loadingStackState} from './stackEditState';

const styles = stylex.create({
  container: {
    minWidth: '500px',
    minHeight: '300px',
  },
  loading: {
    paddingBottom: 'calc(24px + 2 * var(--pad))',
  },
  tab: {
    fontSize: '110%',
    padding: 'var(--halfpad) calc(2 * var(--pad))',
  },
});

/// Show a <Modal /> when editing a stack.
export function MaybeEditStackModal() {
  const loadingState = useAtomValue(loadingStackState);
  const [[stackIntention, stackHashes], setStackIntention] = useAtom(editingStackIntentionHashes);

  const isEditing = stackHashes.size > 0;
  const isLoaded = isEditing && loadingState.state === 'hasValue';

  return isLoaded ? (
    {
      split: () => <LoadedSplitModal />,
      general: () => <LoadedEditStackModal />,
      // TODO: implement absorb model.
      absorb: () => <LoadedAbsorbModal />,
    }[stackIntention]()
  ) : isEditing ? (
    <Modal
      dataTestId="edit-stack-loading"
      dismiss={() => {
        // allow dismissing in loading state in case it gets stuck
        setStackIntention(['general', new Set()]);
      }}>
      <Center
        xstyle={[
          (stackIntention === 'general' || stackIntention === 'absorb') && styles.container,
          styles.loading,
        ]}
        className={stackIntention === 'split' ? 'interactive-split' : undefined}>
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
    <Modal dataTestId="interactive-split-modal" className="split-single-commit-modal-contents">
      <SplitStackEditPanel />
      <Row style={{padding: 'var(--pad) 0', justifyContent: 'flex-end', zIndex: 1}}>
        <StackEditConfirmButtons />
      </Row>
    </Modal>
  );
}

/**
 * A Modal for dedicated absorb UI.
 * While absorbing, the other edit stacks features are unavailable.
 * See `StackStateWithOperationProps.absorbChunks` for details.
 */
function LoadedAbsorbModal() {
  return (
    <Modal dataTestId="interactive-absorb-modal" className="absorb-modal-contents">
      <AbsorbStackEditPanel />
      <Row style={{padding: 'var(--pad) 0', justifyContent: 'flex-end', zIndex: 1}}>
        <StackEditConfirmButtons />
      </Row>
    </Modal>
  );
}

/** A Modal for general stack editing UI. */
function LoadedEditStackModal() {
  const panels = {
    commits: {
      label: t('Commits'),
      render: () => (
        <ScrollY maxSize="calc((100vh / var(--zoom)) - 200px)">
          <StackEditSubTree
            activateSplitTab={() => {
              setActiveTab('split');
              tracker.track('StackEditInlineSplitButton');
            }}
          />
        </ScrollY>
      ),
    },
    split: {
      label: t('Split'),
      render: () => <SplitStackEditPanel />,
    },
    // TODO: re-enable the "files" tab
    // files: {label: t('Files'), render: () => <FileStackEditPanel />},
  } as const;
  type Tab = keyof typeof panels;
  const [activeTab, setActiveTab] = useState<Tab>('commits');

  return (
    <Modal className="edit-stack-modal-contents">
      <Panels
        active={activeTab}
        panels={panels}
        onSelect={tab => {
          setActiveTab(tab);
          tracker.track('StackEditChangeTab', {extras: {tab}});
        }}
        xstyle={styles.container}
        tabXstyle={styles.tab}
      />
      <Row style={{padding: 'var(--pad) 0', justifyContent: 'flex-end'}}>
        {activeTab === 'split' && <SplitStackToolbar />}
        <FlexSpacer />
        <StackEditConfirmButtons />
      </Row>
    </Modal>
  );
}
