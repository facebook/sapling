/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {colorMapAtom} from './diffServiceClient';
import {primerColorModeAtom} from './jotai/atoms';
import {useAtomValue} from 'jotai';
import React, {Suspense, useEffect, useMemo} from 'react';
import {updateTextMateGrammarCSS} from 'shared/textmate-lib/textmateStyles';

/**
 * Component that ensures TextMate syntax highlighting CSS is injected into
 * the DOM. It fetches the colorMap from the diff service worker and calls
 * updateTextMateGrammarCSS() to create CSS rules for the tokenization classes
 * (e.g., .mtk1, .mtk2, etc.).
 */
// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function TextMateStyles(): React.ReactElement | null {
  return (
    <Suspense fallback={null}>
      <TextMateStylesInner />
    </Suspense>
  );
});

function TextMateStylesInner(): React.ReactElement | null {
  const colorMode = useAtomValue(primerColorModeAtom);
  const colorAtom = useMemo(() => colorMapAtom(colorMode), [colorMode]);
  const colors = useAtomValue(colorAtom);

  useEffect(() => {
    updateTextMateGrammarCSS(colors);
  }, [colors]);

  return null;
}
