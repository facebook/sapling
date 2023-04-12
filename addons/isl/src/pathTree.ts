/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoPath} from 'shared/types/common';

export type PathTree<T> = Map<string, PathTree<T> | T>;
/**
 * Path tree reconstructs the tree structure from a set of paths,
 * then compresses path names for single-child directories.
 *
 * ```
 * buidlPathTree({
 *   'a/b/file1.txt': {...},
 *   'a/b/file2.txt': {...},
 *   'a/file3.txt': {...},
 *   'a/d/e/f/file4.txt': {...},
 *   'q/file5.txt': {...},
 * })
 * Map{
 *   a: Map{
 *     b: Map{
 *       'file1.txt' : {...},
 *       'file2.txt' : {...},
 *     },
 *     'file3.txt': {...},
 *     'd/e/f': Map{
 *       'file4.txt': {...}
 *     }
 *   },
 *   q: Map{
 *     'file5.txt': {...},
 *   }
 * }
 * ```
 */
export function buildPathTree<T>(paths: Record<RepoPath, T>): PathTree<T> {
  function recurse(input: Map<string, T>) {
    const intermediateTree: Map<string, Map<string, T>> = new Map();
    const plainFiles = new Map<string, T>();

    // group files by common path
    for (const [path, data] of input.entries()) {
      const [folder] = path.split('/', 1);
      const rest = path.slice(folder.length + 1);
      if (rest === '') {
        // no more folders in this path, use the data directly
        plainFiles.set(folder, data);
      } else if (intermediateTree.has(folder)) {
        const existing = intermediateTree.get(folder);
        existing?.set(rest, data);
      } else {
        intermediateTree.set(folder, new Map([[rest, data]]));
      }
    }

    // recurse into each grouping
    const tree: PathTree<T> = new Map();
    for (const [key, value] of intermediateTree.entries()) {
      const resultTree = recurse(value);
      // if a folder 'a' contains exactly one subfolder 'b', we can collapse it into just 'a/b'
      if (resultTree.size === 1) {
        const [innerkey, inner] = resultTree.entries().next().value;
        if (!(inner instanceof Map)) {
          // the single file is the bottom of the tree, don't absorb it
          tree.set(key, resultTree);
        } else {
          tree.set(key + '/' + innerkey, inner);
        }
      } else {
        tree.set(key, resultTree);
      }
    }

    // re-add the file entries we found
    for (const [key, value] of plainFiles.entries()) {
      tree.set(key, value);
    }

    return tree;
  }

  return recurse(new Map(Object.entries(paths)));
}
