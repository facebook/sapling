/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {createElement} from 'react';

const bodyHTMLClassName = 'PRT-bodyHTML';

/**
 * Renders a string of HTML provided by the `trustedHTML` prop, so the caller
 * is responsible for ensuring `trustedHTML` contains "safe" HTML (i.e., no
 * <script> tags or presentation logic that could distort the page).
 *
 * Using dangerouslySetInnerHTML is generally frowned upon, but in our case, we
 * trust GitHub's GraphQL API to return sanitized HTML via fields such as
 * `bodyHTML` and `titleHTML`, which ensures GitHub issues and users get
 * linkified as they would on GitHub.
 *
 * This component renders the specified `trustedHTML` wrapped in a `<div>` that
 * will style the `trustedHTML` to match the host page, though additional
 * styling can be enforced via the optional `className` prop.
 */
export default function TrustedRenderedMarkdown({
  trustedHTML,
  inline,
  className,
}: {
  trustedHTML: string;
  inline?: boolean;
  className?: string;
}): React.ReactElement {
  const clazz = className != null ? `${className} ${bodyHTMLClassName}` : bodyHTMLClassName;
  // We may want to rewrite URLs to github.com to point to the equivalent URL
  // in our own tool, if it is supported.
  const type = inline ? 'span' : 'div';
  return createElement(type, {className: clazz, dangerouslySetInnerHTML: {__html: trustedHTML}});
}
