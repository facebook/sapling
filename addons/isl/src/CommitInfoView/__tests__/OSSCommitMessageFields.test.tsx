/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {
  commitMessageFieldsToString,
  mergeCommitMessageFields,
  mergeManyCommitMessageFields,
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

  it('handles empty title when coverting to string', () => {
    expect(
      commitMessageFieldsToString(OSSDefaultFieldSchema, {
        Title: '',
        Description: 'my summary\nline 2',
      }),
    ).toEqual(expect.stringMatching(/Temporary Commit at .*\n\nmy summary\nline 2/));
  });

  it('leading spaces in title is OK', () => {
    expect(
      commitMessageFieldsToString(OSSDefaultFieldSchema, {
        Title: '     title',
        Description: 'my summary\nline 2',
      }),
    ).toEqual(
      `     title

my summary
line 2`,
    );
  });

  describe('mergeCommitMessageFields', () => {
    it('can merge fields', () => {
      expect(
        mergeCommitMessageFields(
          OSSDefaultFieldSchema,
          {
            Title: 'Commit A',
            Description: 'Description A',
          },
          {
            Title: 'Commit B',
            Description: 'Description B',
          },
        ),
      ).toEqual({
        Title: 'Commit A, Commit B',
        Description: 'Description A\nDescription B',
      });
    });

    it('leaves identical fields alone', () => {
      expect(
        mergeCommitMessageFields(
          OSSDefaultFieldSchema,
          {
            Title: 'Commit A',
            Description: 'Description A',
          },
          {
            Title: 'Commit A',
            Description: 'Description A',
          },
        ),
      ).toEqual({
        Title: 'Commit A',
        Description: 'Description A',
      });
    });

    it('ignores empty fields', () => {
      expect(
        mergeCommitMessageFields(
          OSSDefaultFieldSchema,
          {
            Title: 'Commit A',
          },
          {
            Title: 'Commit B',
          },
        ),
      ).toEqual({
        Title: 'Commit A, Commit B',
      });
    });
  });

  describe('mergeManyCommitMessageFields', () => {
    it('can merge fields', () => {
      expect(
        mergeManyCommitMessageFields(OSSDefaultFieldSchema, [
          {
            Title: 'Commit A',
            Description: 'Description A',
          },
          {
            Title: 'Commit B',
            Description: 'Description B',
          },
          {
            Title: 'Commit C',
            Description: 'Description C',
          },
        ]),
      ).toEqual({
        Title: 'Commit A, Commit B, Commit C',
        Description: 'Description A\nDescription B\nDescription C',
      });
    });

    it('ignores empty fields', () => {
      expect(
        mergeManyCommitMessageFields(OSSDefaultFieldSchema, [
          {
            Title: 'Commit A',
          },
          {
            Title: 'Commit B',
          },
          {
            Title: 'Commit C',
          },
        ]),
      ).toEqual({
        Title: 'Commit A, Commit B, Commit C',
      });
    });
  });
});
