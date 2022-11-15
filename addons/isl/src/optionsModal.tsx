/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Deferred} from 'shared/utils';

import {useCommand} from './ISLShortcuts';
import {Icon} from './Icon';
import {Modal} from './Modal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilState, useSetRecoilState} from 'recoil';
import {defer} from 'shared/utils';

import './optionsModal.css';

type ModalConfig<T> = {
  type: 'confirm';
  buttons: ReadonlyArray<T>;
  title: React.ReactNode;
  message: React.ReactNode;
};
type ModalState<T> = {
  config: ModalConfig<T>;
  visible: boolean;
  deferred: Deferred<T | undefined>;
};

const modalState = atom<ModalState<string> | null>({
  key: 'modalState',
  default: null,
});

export function OptionsModal() {
  const [modal, setModal] = useRecoilState(modalState);

  const dismiss = () => {
    if (modal?.visible) {
      modal.deferred.resolve(undefined);
      setModal({...modal, visible: false});
    }
  };

  useCommand('Escape', dismiss);

  if (modal?.visible) {
    return (
      <Modal
        width="500px"
        height="fit-content"
        className="options-modal"
        aria-labelledby="options-modal-title"
        aria-describedby="options-modal-message">
        <div className="options-modal-header">
          <VSCodeButton appearance="icon" onClick={dismiss}>
            <Icon icon="x" />
          </VSCodeButton>
        </div>
        <div id="options-modal-title">{modal.config.title}</div>
        <div id="options-modal-message">{modal.config.message}</div>
        <div className="options-modal-buttons">
          {modal.config.buttons.map(button => (
            <VSCodeButton
              appearance="secondary"
              onClick={() => {
                modal.deferred.resolve(button);
                setModal({...modal, visible: false});
              }}
              key={button}>
              {button}
            </VSCodeButton>
          ))}
        </div>
      </Modal>
    );
  }

  return null;
}

/**
 * Hook that provides a callback to show a modal with a custom set of buttons.
 * Modal has a dismiss button & dismisses on Escape keypress, thus you must always be able to handle
 * returning `undefined`.
 *
 * For now, we assume all uses of useOptionModal are triggerred directly from a user action.
 * If that's not the case, it would be possible to have multiple modals overlap.
 **/
export function useOptionModal(): <T extends string>(
  config: ModalConfig<T>,
) => Promise<T | undefined> {
  const setModal = useSetRecoilState(modalState);

  return <T extends string>(config: ModalConfig<T>) => {
    const deferred = defer<string | undefined>();
    setModal({
      config,
      visible: true,
      deferred,
    });

    return deferred.promise as Promise<T>;
  };
}
