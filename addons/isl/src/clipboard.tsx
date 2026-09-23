/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * HTML <a> tag for `text` pointing to `url`. Useful for copying rich text links.
 */
export function clipboardLinkHtml(text: string, url: string): string {
  return `<a href="${url}">${text}</a>`;
}
