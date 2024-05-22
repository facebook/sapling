/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoRelativePath} from './types';
import type {ContextMenuItem} from 'shared/ContextMenu';

import serverAPI from './ClientToServerAPI';
import {Row} from './ComponentUtils';
import {availableCwds} from './CwdSelector';
import {Subtle} from './Subtle';
import {Button} from './components/Button';
import {T, t} from './i18n';
import {writeAtom} from './jotaiUtils';
import foundPlatform from './platform';
import {registerCleanup, registerDisposable} from './utils';
import {atom, useAtomValue} from 'jotai';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';

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

  const menu = useContextMenu(() => {
    const fullPaths = unsaved.map(({path}) => path);
    const disambiguated = minimalDisambiguousPaths(fullPaths);
    const options: Array<ContextMenuItem> = disambiguated.map((name, i) => ({
      label: t('Open $name', {replace: {$name: name}}),
      onClick: () => {
        foundPlatform.openFile(fullPaths[i]);
      },
    }));
    return options;
  });

  if (unsaved.length === 0) {
    return null;
  }
  return (
    <Subtle>
      <Row>
        <T count={unsaved.length}>unsavedFileCount</T>
        <Button icon>
          <Icon icon="ellipsis" onClick={menu} />
        </Button>
      </Row>
    </Subtle>
  );
}
