/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath} from './types';

import serverAPI from './ClientToServerAPI';
import {Row} from './ComponentUtils';
import {availableCwds} from './CwdSelector';
import {Subtle} from './Subtle';
import {T} from './i18n';
import {writeAtom} from './jotaiUtils';
import {registerCleanup, registerDisposable} from './utils';
import {atom, useAtomValue} from 'jotai';

/**
 * A list of files for this repo that are unsaved in the IDE.
 * This is always `[]` for unsupported platforms like browser.
 */
export const unsavedFiles = atom<Array<{path: RepoRelativePath; uri: string}>>([]);
registerCleanup(
  availableCwds,
  serverAPI.onConnectOrReconnect(() => {
    serverAPI.postMessage({
      type: 'platform/subscribeToUnsavedFiles',
    });
  }),
  import.meta.hot,
);
registerDisposable(
  availableCwds,
  serverAPI.onMessageOfType('platform/unsavedFiles', event =>
    writeAtom(unsavedFiles, event.unsaved),
  ),
  import.meta.hot,
);

export function UnsavedFilesCount() {
  const unsaved = useAtomValue(unsavedFiles);

  if (unsaved.length === 0) {
    return null;
  }
  return (
    <Subtle>
      <Row>
        <T count={unsaved.length}>unsavedFileCount</T>
      </Row>
    </Subtle>
  );
}
