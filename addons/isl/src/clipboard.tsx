/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import platform from './platform';

/** Copy text to the clipboard */
export function clipboardCopyText(text: string) {
  return platform.clipboardCopy(text);
}

/**
 * Copy text that refers to a URL to the clipboard.
 * If pasted into a rich text input, it will paste as text with a URL link.
 * If pasted into a plain text input, it will just use the text without the url.
 */
export function clipboardCopyLink(text: string, url: string) {
  return platform.clipboardCopy(text, clipboardLinkHtml(text, url));
}

/**
 * HTML <a> tag for `text` pointing to `url`. Useful for copying rich text links.
 */
export function clipboardLinkHtml(text: string, url: string): string {
  return `<a href="${url}">${text}</a>`;
}
