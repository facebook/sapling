/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {colorMap} from './diffServiceClient';
import {primerColorModeAtom} from './jotai/atoms';
import {useAtomValue} from 'jotai';
import React, {useEffect} from 'react';
import {useRecoilValueLoadable} from 'recoil';
import {updateTextMateGrammarCSS} from 'shared/textmate-lib/textmateStyles';

/**
 * Component that ensures TextMate syntax highlighting CSS is injected into
 * the DOM. It fetches the colorMap from the diff service worker and calls
 * updateTextMateGrammarCSS() to create CSS rules for the tokenization classes
 * (e.g., .mtk1, .mtk2, etc.).
 *
 * This component must be rendered within <RecoilRoot> since it uses Recoil
 * selectors to communicate with the diff service worker.
 */
// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function TextMateStyles(): React.ReactElement | null {
  const colorMode = useAtomValue(primerColorModeAtom);
  const colorMapLoadable = useRecoilValueLoadable(colorMap(colorMode));

  useEffect(() => {
    if (colorMapLoadable.state === 'hasValue') {
      updateTextMateGrammarCSS(colorMapLoadable.contents);
    }
  }, [colorMapLoadable]);

  return null;
});
