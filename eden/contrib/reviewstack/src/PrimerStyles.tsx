/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Theme} from '@primer/react/lib/ThemeProvider';

import {useTheme} from '@primer/react';
import React from 'react';

/**
 * React component that dynamically generates a <style> element using values
 * from the active Primer theme. Normally, we can rely on specifying the `sx`
 * prop for our React components, but for <BodyHTML>, we get the HTML as an
 * opaque string, so we cannot leverage `sx`.
 */
// eslint-disable-next-line prefer-arrow-callback
export default React.memo(function PrimerStyles(): React.ReactElement {
  const {theme} = useTheme();
  return (
    <style>
      {`
${defineStyleOnBody(theme)}

.PRT-bodyHTML p {
  margin-top: 0;
  margin-bottom: 16px;
}

.PRT-review-comment-text {
  font-size: 13px;
  line-height: 16px;
}

.PRT-bodyHTML a {
  color: ${theme?.colors.accent.fg}
}

.PRT-bodyHTML pre, .PRT-bodyHTML code {
  font-family: ${theme?.fonts.mono};
}

.PRT-bodyHTML pre {
  background-color: ${theme?.colors.canvas.subtle};
  border-radius: 6px;
  font-size: 85%;
  line-height: 1.45;
  margin-bottom: 16px;
  padding: 16px;
}

/* Intended for inline code elements. */
.PRT-bodyHTML code {
  background-color: ${theme?.colors.neutral.muted};
  border-radius: 6px;
  font-size: 85%;
  padding: 0.2em 0.4em;
}

/**
 * Because code blocks are rendered as <pre><code /></pre>, we have to override
 * a number of styles for inline <code> elements when in a <pre> code block.
 */
.PRT-bodyHTML pre > code {
  background-color: transparent;
  border: 0;
  font-size: 100%;
  padding: 0;
}

.PRT-bodyHTML blockquote {
  border-left: 0.25em solid ${theme?.colors.border.default};
  color: ${theme?.colors.fg.muted};
  margin-block-start: 1em;
  margin-block-end: 1em;
  margin-inline-start: 40px;
  margin-inline-end: 40px;
  padding: 0 1em;
}

.PRT-bodyHTML > *:last-child {
  margin-bottom: 0 !important;
}

/* GitHub suggested change styling */
.PRT-bodyHTML .blob-code-deletion,
.PRT-bodyHTML .blob-code-marker-deletion {
  background-color: ${theme?.colors.diffBlob.deletion.lineBg};
}

.PRT-bodyHTML .blob-code-addition,
.PRT-bodyHTML .blob-code-marker-addition {
  background-color: ${theme?.colors.diffBlob.addition.lineBg};
}

.PRT-bodyHTML .blob-num-deletion {
  background-color: ${theme?.colors.diffBlob.deletion.numBg};
  color: ${theme?.colors.diffBlob.deletion.numText};
}

.PRT-bodyHTML .blob-num-addition {
  background-color: ${theme?.colors.diffBlob.addition.numBg};
  color: ${theme?.colors.diffBlob.addition.numText};
}

/* Style the suggestion container */
.PRT-bodyHTML .js-suggested-changes-blob,
.PRT-bodyHTML .diff-view {
  border: 1px solid ${theme?.colors.border.default};
  border-radius: 6px;
  overflow: hidden;
  margin: 8px 0;
  /* Use flexbox to collapse whitespace text nodes between elements */
  display: flex;
  flex-direction: column;
}

/* Reduce padding in suggestion header - style like GitHub */
.PRT-bodyHTML .js-suggested-changes-blob > div:first-child {
  padding: 8px 10px;
  background-color: ${theme?.colors.canvas.subtle};
  border-bottom: 1px solid ${theme?.colors.border.default};
  font-size: 12px;
  color: ${theme?.colors.fg.muted};
  /* Collapse whitespace inside header */
  display: flex;
  flex-direction: column;
}

/* Collapse whitespace around "Suggested change" text */
.PRT-bodyHTML .js-suggested-changes-blob > div:first-child .color-fg-muted {
  display: inline;
}

/* Remove excessive padding in blob wrapper */
.PRT-bodyHTML .js-suggested-changes-blob .blob-wrapper {
  padding: 0;
  /* Collapse whitespace inside blob wrapper */
  display: flex;
  flex-direction: column;
}

/* Hide empty js-apply-changes div */
.PRT-bodyHTML .js-suggested-changes-blob .js-apply-changes:empty {
  display: none;
}

/* Ensure table fills width */
.PRT-bodyHTML .js-suggested-changes-blob table {
  width: 100%;
  border-collapse: collapse;
}

.PRT-bodyHTML .js-suggested-changes-blob td {
  font-family: ${theme?.fonts.mono};
  font-size: 12px;
  line-height: 20px;
}

/* Style line number cells */
.PRT-bodyHTML .js-suggested-changes-blob .blob-num {
  width: 1%;
  min-width: 40px;
  padding: 0 10px;
  text-align: right;
  vertical-align: top;
  /* Hide the unhelpful "·" character */
  font-size: 0;
}

/* Style code cells with proper padding */
.PRT-bodyHTML .js-suggested-changes-blob .blob-code-inner {
  padding: 0 10px;
  white-space: pre-wrap;
  word-break: break-all;
}

/* Word-level diff highlighting for changed characters */
.PRT-bodyHTML .js-suggested-changes-blob .x {
  background-color: ${theme?.colors.diffBlob.addition.wordBg};
}

.PRT-bodyHTML .js-suggested-changes-blob .blob-code-deletion .x {
  background-color: ${theme?.colors.diffBlob.deletion.wordBg};
}

.reviewstack .drawer-label {
  background-color: ${theme?.colors.neutral.muted};
}

.reviewstack {
  --panel-view-border: ${theme?.colors.border.default};
}
`}
    </style>
  );
});

/**
 * Defining a style on <body> admittedly makes <App> "less portable" because it
 * imposes requirements on the look of the host page, but this is the most
 * straightforward way to ensure things look right when <App> is less than the
 * height of the full page. We can make this an option on the props for <App>
 * if it becomes an issue.
 */
function defineStyleOnBody(theme: Theme | undefined) {
  return `\
body {
  background-color: ${theme?.colors.canvas.default};
}`;
}
