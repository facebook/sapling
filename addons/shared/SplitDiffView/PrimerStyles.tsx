/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useTheme} from '@primer/react';
import React from 'react';

/**
 * React component that dynamically generates a <style> element using values
 * from the active Primer theme.
 */
// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PrimerStyles(): React.ReactElement {
  const {theme} = useTheme();
  return (
    <style>
      {`
.patch-word-begin {
  border-top-left-radius: .2em;
  border-bottom-left-radius: .2em;
}

.patch-word-end {
  border-top-right-radius: .2em;
  border-bottom-right-radius: .2em;
}

.patch-add-line {
  background-color: ${theme?.colors.diffBlob.addition.lineBg};
}

.patch-add-line-number {
  color: ${theme?.colors.diffBlob.addition.numText};
  background-color: ${theme?.colors.diffBlob.addition.numBg};
}

.patch-add-word {
  background-color: ${theme?.colors.diffBlob.addition.wordBg};
}

.patch-remove-line {
  background-color: ${theme?.colors.diffBlob.deletion.lineBg};
}

.patch-remove-line-number {
  color: ${theme?.colors.diffBlob.deletion.numText};
  background-color: ${theme?.colors.diffBlob.deletion.numBg};
}

.patch-remove-word {
  background-color: ${theme?.colors.diffBlob.deletion.wordBg};
}

.patch-expanded, .patch-expanded-number {
  background-color: ${theme?.colors.canvas.subtle};
}

.patch-expanded-number {
  color: ${theme?.colors.fg.subtle};
}

.SplitDiffView-hunk-table td {
  font-family: ${theme?.fonts.mono}
}

/**
 * pl = prettylights theme
 *
 * Mapping from CSS class name to Primer color deduced from a combination of:
 * https://cdn.jsdelivr.net/npm/github-syntax-theme-generator@0.5.0/build/css/github-light.css
 * https://primer.style/react/theme-reference
 *
 * Though it is a bit confusing because github-light.css defines some classes
 * multiple times, such as .pl-s.
 */

.pl-ba {
  color: ${theme?.colors.prettylights.syntax.brackethighlighterAngle};
}

.pl-bu {
  color: ${theme?.colors.prettylights.syntax.brackethighlighterUnmatched};
}

.pl-c {
  color: ${theme?.colors.prettylights.syntax.comment};
}

.pl-c1 {
  color: ${theme?.colors.prettylights.syntax.constant};
}

.pl-c2 {
  background-color: ${theme?.colors.prettylights.syntax.carriageReturnBg};
  color: ${theme?.colors.prettylights.syntax.carriageReturnText};
}

.pl-c2::before {
  content: "^M";
}

.pl-corl {
  color: ${theme?.colors.prettylights.syntax.constantOtherReferenceLink};
  text-decoration: underline;
}

.pl-e, .pl-en {
  color: ${theme?.colors.prettylights.syntax.entity};
}

.pl-ent {
  color: ${theme?.colors.prettylights.syntax.entityTag};
}

.pl-ii {
  background-color: ${theme?.colors.prettylights.syntax.invalidIllegalBg};
  color: ${theme?.colors.prettylights.syntax.invalidIllegalText};
}

.pl-k {
  color: ${theme?.colors.prettylights.syntax.keyword};
}

.pl-s, .pl-pds {
  color: ${theme?.colors.prettylights.syntax.string};
}

.pl-sg {
  color: ${theme?.colors.prettylights.syntax.sublimelinterGutterMark};
}

.pl-smi {
  color: ${theme?.colors.prettylights.syntax.storageModifierImport};
}

.pl-sr {
  color: ${theme?.colors.prettylights.syntax.stringRegexp};
  font-weight: bold;
}

.pl-v {
  color: ${theme?.colors.prettylights.syntax.variable};
}
`}
    </style>
  );
});
