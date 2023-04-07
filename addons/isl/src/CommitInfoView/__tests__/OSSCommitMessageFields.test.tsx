/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  commitMessageFieldsToString,
  OSSDefaultFieldSchema,
  parseCommitMessageFields,
} from '../CommitMessageFields';

describe('InternalCommitInfoFields', () => {
  it('parses messages correctly', () => {
    const parsed = parseCommitMessageFields(
      OSSDefaultFieldSchema,
      'my title',
      `My description!
another line
`,
    );

    expect(parsed.Title).toEqual('my title');
    expect(parsed.Description).toEqual('My description!\nanother line\n');
  });

  it('converts to string properly', () => {
    expect(
      commitMessageFieldsToString(OSSDefaultFieldSchema, {
        Title: 'my title',
        Description: 'my summary\nline 2',
      }),
    ).toEqual(
      `my title

my summary
line 2`,
    );
  });
});
