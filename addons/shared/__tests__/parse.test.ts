/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {parsePatch} from '../patch/parse';

describe('patch/parse', () => {
  it('should parse basic modified patch', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/a
--- sapling/eden/scm/a
+++ sapling/eden/scm/a
@@ -1,1 +1,2 @@
 1
+2
`;
    const expected = [
      {
        hunks: [
          {
            linedelimiters: ['\n', '\n'],
            lines: [' 1', '+2'],
            newLines: 2,
            newStart: 1,
            oldLines: 1,
            oldStart: 1,
          },
        ],
        newFileName: 'sapling/eden/scm/a',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Modified',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse rename', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/b
rename from sapling/eden/scm/a
rename to sapling/eden/scm/b
`;
    const expected = [
      {
        hunks: [],
        newFileName: 'sapling/eden/scm/b',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Renamed',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse rename and modify', () => {
    const patch = `
diff --git sapling/eden/addons/LICENSE sapling/eden/addons/LICENSE.bak
rename from sapling/eden/addons/LICENSE
rename to sapling/eden/addons/LICENSE.bak
--- sapling/eden/addons/LICENSE
+++ sapling/eden/addons/LICENSE.bak
@@ -2,6 +2,7 @@

 Copyright (c) Meta Platforms, Inc. and its affiliates.

+
`;
    const expected = [
      {
        hunks: [
          {
            linedelimiters: ['\n', '\n', '\n', '\n'],
            lines: ['', ' Copyright (c) Meta Platforms, Inc. and its affiliates.', '', '+'],
            newLines: 7,
            newStart: 2,
            oldLines: 6,
            oldStart: 2,
          },
        ],
        newFileName: 'sapling/eden/addons/LICENSE.bak',
        oldFileName: 'sapling/eden/addons/LICENSE',
        type: 'Renamed',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse new file', () => {
    const patch = `
diff --git sapling/eden/scm/c sapling/eden/scm/c
new file mode 100644
--- /dev/null
+++ sapling/eden/scm/c
@@ -0,0 +1,1 @@
+1
`;
    const expected = [
      {
        hunks: [
          {
            linedelimiters: ['\n'],
            lines: ['+1'],
            newLines: 1,
            newStart: 1,
            oldLines: 0,
            oldStart: 1,
          },
        ],
        newFileName: 'sapling/eden/scm/c',
        newMode: '100644',
        oldFileName: 'sapling/eden/scm/c',
        type: 'Added',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse new empty file', () => {
    const patch = `
diff --git sapling/eden/addons/d sapling/eden/addons/d
new file mode 100644
`;
    const expected = [
      {
        hunks: [],
        newFileName: 'sapling/eden/addons/d',
        newMode: '100644',
        oldFileName: 'sapling/eden/addons/d',
        type: 'Added',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse deleted file', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/a
deleted file mode 100644
--- sapling/eden/scm/a
+++ /dev/null
@@ -1,1 +0,0 @@
-1
`;
    const expected = [
      {
        hunks: [
          {
            linedelimiters: ['\n'],
            lines: ['-1'],
            newLines: 0,
            newStart: 1,
            oldLines: 1,
            oldStart: 1,
          },
        ],
        newFileName: 'sapling/eden/scm/a',
        newMode: '100644',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Removed',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse copied file', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/b
copy from sapling/eden/scm/a
copy to sapling/eden/scm/b
`;
    const expected = [
      {
        hunks: [],
        newFileName: 'sapling/eden/scm/b',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Copied',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse multiple files', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/a
--- sapling/eden/scm/a
+++ sapling/eden/scm/a
@@ -1,1 +1,2 @@
 1
+2
diff --git sapling/eden/scm/a sapling/eden/scm/b
copy from sapling/eden/scm/a
copy to sapling/eden/scm/b
diff --git sapling/eden/scm/c sapling/eden/scm/d
copy from sapling/eden/scm/c
copy to sapling/eden/scm/d
`;
    const expected = [
      {
        hunks: [
          {
            linedelimiters: ['\n', '\n'],
            lines: [' 1', '+2'],
            newLines: 2,
            newStart: 1,
            oldLines: 1,
            oldStart: 1,
          },
        ],
        newFileName: 'sapling/eden/scm/a',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Modified',
      },
      {
        hunks: [],
        newFileName: 'sapling/eden/scm/b',
        oldFileName: 'sapling/eden/scm/a',
        type: 'Copied',
      },
      {
        hunks: [],
        newFileName: 'sapling/eden/scm/d',
        oldFileName: 'sapling/eden/scm/c',
        type: 'Copied',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should parse file mode change', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/a
old mode 100644
new mode 100755
`;
    const expected = [
      {
        hunks: [],
        newFileName: 'sapling/eden/scm/a',
        newMode: '100755',
        oldFileName: 'sapling/eden/scm/a',
        oldMode: '100644',
        type: 'Modified',
      },
    ];
    expect(parsePatch(patch)).toEqual(expected);
  });

  it('should fail for invalid file mode format', () => {
    const patch = `
diff --git sapling/eden/scm/a sapling/eden/scm/a
old mode XXX
new mode 100755
`;
    expect(() => parsePatch(patch)).toThrow("invalid format 'old mode XXX'");
  });
});
