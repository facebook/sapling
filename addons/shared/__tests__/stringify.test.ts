/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {filterFilesFromPatch, parsePatch} from '../patch/parse';
import {stringifyPatch} from '../patch/stringify';

describe('patch/stringify', () => {
  describe('round-trip conversion', () => {
    it('should round-trip basic modified patch', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/a
--- sapling/eden/scm/a
+++ sapling/eden/scm/a
@@ -1,1 +1,2 @@
 1
+2
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip rename', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/b
rename from sapling/eden/scm/a
rename to sapling/eden/scm/b
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip rename and modify', () => {
      const patch = `diff --git sapling/eden/addons/LICENSE sapling/eden/addons/LICENSE.bak
rename from sapling/eden/addons/LICENSE
rename to sapling/eden/addons/LICENSE.bak
--- sapling/eden/addons/LICENSE
+++ sapling/eden/addons/LICENSE.bak
@@ -2,6 +2,7 @@

 Copyright (c) Meta Platforms, Inc. and its affiliates.

+
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip new file', () => {
      const patch = `diff --git sapling/eden/scm/c sapling/eden/scm/c
new file mode 100644
--- /dev/null
+++ sapling/eden/scm/c
@@ -0,0 +1,1 @@
+1
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip new empty file', () => {
      const patch = `diff --git sapling/eden/addons/d sapling/eden/addons/d
new file mode 100644
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip deleted file', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/a
deleted file mode 100644
--- sapling/eden/scm/a
+++ /dev/null
@@ -1,1 +0,0 @@
-1
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip copied file', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/b
copy from sapling/eden/scm/a
copy to sapling/eden/scm/b
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip multiple files', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/a
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
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip file mode change', () => {
      const patch = `diff --git sapling/eden/scm/a sapling/eden/scm/a
old mode 100644
new mode 100755
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip submodule modification', () => {
      const patch = `diff --git a/external/brotli b/external/brotli
--- a/external/brotli
+++ b/external/brotli
@@ -1,1 +1,1 @@
-Subproject commit 892110204ccf44fcd493ae415c9a69c470c2a9cf
+Subproject commit 57de5cc4288565a9c3a7af978ef15f0abf0ada1b
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should round-trip added submodule', () => {
      const patch = `diff --git a/path/to/submodule b/path/to/submodule
new file mode 160000
--- /dev/null
+++ b/path/to/submodule
@@ -0,0 +1,1 @@
+Subproject commit 7ef4220022059b9b1e1d8ec4eea6f7abd011894f
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });
  });

  describe('hunk range formatting', () => {
    it('should format single line hunk with count', () => {
      const patch = `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -5,1 +5,2 @@
 line 5
+new line
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should format empty old range', () => {
      const patch = `diff --git sapling/eden/scm/c sapling/eden/scm/c
new file mode 100644
--- /dev/null
+++ sapling/eden/scm/c
@@ -0,0 +1,3 @@
+line 1
+line 2
+line 3
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });

    it('should format empty new range', () => {
      const patch = `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +0,0 @@
-line 1
-line 2
-line 3
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });
  });

  describe('line delimiter handling', () => {
    it('should preserve hunk line delimiters', () => {
      const patch = `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,1 +1,2 @@
 line 1\r
+line 2\r
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      // Header lines use standard \n, but hunk content preserves \r
      expect(stringified).toEqual(patch);
    });
  });

  describe('multiple hunks', () => {
    it('should handle multiple hunks in a single file', () => {
      const patch = `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 line 1
+new line 1.5
 line 2
 line 3
@@ -10,2 +11,3 @@
 line 10
+new line 10.5
 line 11
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });
  });

  describe('no newline at end of file', () => {
    it('should handle backslash-no-newline marker', () => {
      const patch = `diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,1 +1,1 @@
-old content
\\ No newline at end of file
+new content
\\ No newline at end of file
`;
      const parsed = parsePatch(patch);
      const stringified = stringifyPatch(parsed);
      expect(stringified).toEqual(patch);
    });
  });
});

describe('patch/filterFilesFromPatch', () => {
  describe('basic filtering', () => {
    it('should filter out a single modified file', () => {
      const patch = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
diff --git a/remove.ts b/remove.ts
--- a/remove.ts
+++ b/remove.ts
@@ -1,1 +1,2 @@
 original
+changed
`;
      const filtered = filterFilesFromPatch(patch, ['a/remove.ts']);
      const expected = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      expect(filtered).toEqual(expected);
    });

    it('should filter out multiple files', () => {
      const patch = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
diff --git a/remove1.ts b/remove1.ts
--- a/remove1.ts
+++ b/remove1.ts
@@ -1,1 +1,1 @@
-old
+new
diff --git a/remove2.ts b/remove2.ts
--- a/remove2.ts
+++ b/remove2.ts
@@ -1,1 +1,1 @@
-old
+new
`;
      const filtered = filterFilesFromPatch(patch, ['a/remove1.ts', 'b/remove2.ts']);
      const expected = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      expect(filtered).toEqual(expected);
    });

    it('should return empty string when all files are filtered', () => {
      const patch = `diff --git a/remove.ts b/remove.ts
--- a/remove.ts
+++ b/remove.ts
@@ -1,1 +1,2 @@
 original
+changed
`;
      const filtered = filterFilesFromPatch(patch, ['a/remove.ts']);
      expect(filtered).toEqual('');
    });

    it('should return original patch when no files match filter', () => {
      const patch = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['a/nonexistent.ts']);
      expect(filtered).toEqual(patch);
    });
  });

  describe('path prefix handling', () => {
    it('should filter files with a/ prefix', () => {
      const patch = `diff --git a/file.ts b/file.ts
--- a/file.ts
+++ b/file.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['a/file.ts']);
      expect(filtered).toEqual('');
    });

    it('should filter files with b/ prefix', () => {
      const patch = `diff --git a/file.ts b/file.ts
--- a/file.ts
+++ b/file.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['b/file.ts']);
      expect(filtered).toEqual('');
    });

    it('should filter files without prefix', () => {
      const patch = `diff --git a/file.ts b/file.ts
--- a/file.ts
+++ b/file.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['file.ts']);
      expect(filtered).toEqual('');
    });

    it('should handle paths with slashes', () => {
      const patch = `diff --git a/src/components/App.tsx b/src/components/App.tsx
--- a/src/components/App.tsx
+++ b/src/components/App.tsx
@@ -1,1 +1,2 @@
 import React from 'react';
+import {useState} from 'react';
`;
      const filtered = filterFilesFromPatch(patch, ['src/components/App.tsx']);
      expect(filtered).toEqual('');
    });
  });

  describe('special file operations', () => {
    it('should filter renamed files by old name', () => {
      const patch = `diff --git a/old.ts b/new.ts
rename from a/old.ts
rename to b/new.ts
--- a/old.ts
+++ b/new.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['a/old.ts']);
      expect(filtered).toEqual('');
    });

    it('should filter renamed files by new name', () => {
      const patch = `diff --git a/old.ts b/new.ts
rename from a/old.ts
rename to b/new.ts
--- a/old.ts
+++ b/new.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, ['b/new.ts']);
      expect(filtered).toEqual('');
    });

    it('should filter new files', () => {
      const patch = `diff --git a/existing.ts b/existing.ts
--- a/existing.ts
+++ b/existing.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
diff --git a/new.ts b/new.ts
new file mode 100644
--- /dev/null
+++ b/new.ts
@@ -0,0 +1,1 @@
+new content
`;
      const filtered = filterFilesFromPatch(patch, ['new.ts']);
      const expected = `diff --git a/existing.ts b/existing.ts
--- a/existing.ts
+++ b/existing.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      expect(filtered).toEqual(expected);
    });

    it('should filter deleted files', () => {
      const patch = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
diff --git a/deleted.ts b/deleted.ts
deleted file mode 100644
--- a/deleted.ts
+++ /dev/null
@@ -1,1 +0,0 @@
-old content
`;
      const filtered = filterFilesFromPatch(patch, ['deleted.ts']);
      const expected = `diff --git a/keep.ts b/keep.ts
--- a/keep.ts
+++ b/keep.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      expect(filtered).toEqual(expected);
    });

    it('should filter copied files', () => {
      const patch = `diff --git a/original.ts b/copy.ts
copy from a/original.ts
copy to b/copy.ts
`;
      const filtered = filterFilesFromPatch(patch, ['copy.ts']);
      expect(filtered).toEqual('');
    });

    it('should filter mode changes', () => {
      const patch = `diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
`;
      const filtered = filterFilesFromPatch(patch, ['script.sh']);
      expect(filtered).toEqual('');
    });
  });

  describe('real-world use case: filtering generated files', () => {
    it('should remove generated code files from patch', () => {
      const patch = `diff --git a/src/manual.ts b/src/manual.ts
--- a/src/manual.ts
+++ b/src/manual.ts
@@ -1,3 +1,4 @@
 // Manually written code
 export function doSomething() {
+  console.log('new feature');
 }
diff --git a/generated/types.ts b/generated/types.ts
--- a/generated/types.ts
+++ b/generated/types.ts
@@ -1,100 +1,200 @@
-// Auto-generated - do not edit
+// Auto-generated - do not edit
+// Lots of noisy changes
 export type GeneratedType = string;
diff --git a/generated/schema.ts b/generated/schema.ts
--- a/generated/schema.ts
+++ b/generated/schema.ts
@@ -1,50 +1,100 @@
-// Auto-generated
+// Auto-generated
+// More noise
 export const schema = {};
`;
      const filtered = filterFilesFromPatch(patch, ['generated/types.ts', 'generated/schema.ts']);
      const expected = `diff --git a/src/manual.ts b/src/manual.ts
--- a/src/manual.ts
+++ b/src/manual.ts
@@ -1,3 +1,4 @@
 // Manually written code
 export function doSomething() {
+  console.log('new feature');
 }
`;
      expect(filtered).toEqual(expected);
    });
  });

  describe('edge cases', () => {
    it('should handle empty patch', () => {
      const filtered = filterFilesFromPatch('', ['file.ts']);
      expect(filtered).toEqual('');
    });

    it('should handle empty file list', () => {
      const patch = `diff --git a/file.ts b/file.ts
--- a/file.ts
+++ b/file.ts
@@ -1,1 +1,2 @@
 line 1
+line 2
`;
      const filtered = filterFilesFromPatch(patch, []);
      expect(filtered).toEqual(patch);
    });

    it('should handle files with multiple hunks', () => {
      const patch = `diff --git a/file.ts b/file.ts
--- a/file.ts
+++ b/file.ts
@@ -1,3 +1,4 @@
 line 1
+new line
 line 2
 line 3
@@ -10,2 +11,3 @@
 line 10
+another new line
 line 11
`;
      const filtered = filterFilesFromPatch(patch, ['file.ts']);
      expect(filtered).toEqual('');
    });
  });
});
