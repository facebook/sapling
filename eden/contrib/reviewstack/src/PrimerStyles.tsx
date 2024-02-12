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
