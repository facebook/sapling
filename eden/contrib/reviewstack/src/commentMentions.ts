/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type ActiveMention = {
  start: number;
  end: number;
  query: string;
};

/**
 * Finds the mention being edited at the cursor. GitHub logins contain only
 * letters, numbers, and hyphens. Requiring whitespace or punctuation before
 * the `@` keeps email addresses from opening the suggestions menu.
 */
export function getActiveMention(text: string, cursor: number): ActiveMention | null {
  if (cursor < 0 || cursor > text.length) {
    return null;
  }

  const beforeCursor = text.slice(0, cursor);
  const match = beforeCursor.match(/(?:^|[\s([{])@([A-Za-z0-9-]*)$/);
  if (match == null) {
    return null;
  }

  const query = match[1];
  const start = cursor - query.length - 1;
  const remainingLogin = text.slice(cursor).match(/^[A-Za-z0-9-]*/)?.[0] ?? '';
  return {start, end: cursor + remainingLogin.length, query};
}

export function replaceActiveMention(
  text: string,
  mention: ActiveMention,
  login: string,
): {text: string; cursor: number} {
  const before = text.slice(0, mention.start);
  const after = text.slice(mention.end);
  const needsTrailingSpace = after === '' || /^[A-Za-z0-9@]/.test(after);
  const replacement = `@${login}${needsTrailingSpace ? ' ' : ''}`;
  return {
    text: `${before}${replacement}${after}`,
    cursor: before.length + replacement.length,
  };
}
