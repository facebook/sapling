/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {getActiveMention, replaceActiveMention} from './commentMentions';

describe('getActiveMention', () => {
  test.each([
    ['@', 1, {start: 0, end: 1, query: ''}],
    ['hello @ti', 9, {start: 6, end: 9, query: 'ti'}],
    ['hello (@octo', 12, {start: 7, end: 12, query: 'octo'}],
    ['@first and @sec', 15, {start: 11, end: 15, query: 'sec'}],
  ])('finds a mention in %p', (text, cursor, expected) => {
    expect(getActiveMention(text, cursor)).toEqual(expected);
  });

  test('includes the rest of a login when the cursor is inside it', () => {
    expect(getActiveMention('hello @octocat!', 10)).toEqual({
      start: 6,
      end: 14,
      query: 'oct',
    });
  });

  test.each([
    ['person@example.com', 14],
    ['hello @octo cat', 15],
    ['plain text', 5],
  ])('does not find an inactive mention in %p', (text, cursor) => {
    expect(getActiveMention(text, cursor)).toBeNull();
  });
});

describe('replaceActiveMention', () => {
  test('inserts a mention and a space at the end of the comment', () => {
    expect(
      replaceActiveMention('hello @ti', {start: 6, end: 9, query: 'ti'}, 'tina'),
    ).toEqual({text: 'hello @tina ', cursor: 12});
  });

  test('replaces the whole login when the cursor is inside it', () => {
    expect(
      replaceActiveMention('hello @octocat!', {start: 6, end: 14, query: 'oct'}, 'octavia'),
    ).toEqual({text: 'hello @octavia!', cursor: 14});
  });

  test('preserves existing whitespace and punctuation', () => {
    expect(
      replaceActiveMention('hello @ti, welcome', {start: 6, end: 9, query: 'ti'}, 'tina'),
    ).toEqual({text: 'hello @tina, welcome', cursor: 11});
  });

  test('separates a mention from adjacent text', () => {
    expect(replaceActiveMention('@tiworks', {start: 0, end: 3, query: 'ti'}, 'tina')).toEqual({
      text: '@tina works',
      cursor: 6,
    });
  });
});
