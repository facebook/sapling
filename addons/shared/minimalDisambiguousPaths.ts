/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

type PathObject = {
  depth: number | null;
  parts: string[];
  rootPrefix: string;
  separator: string;
  hadLeadingSeparator: boolean;
};

type Options = {
  alwaysShowLeadingSeparator?: boolean;
  maxDepth?: number;
};

/**
   * Computes the minimally differentiable display path for each file.
   *
   * The algorithm is O(n*m^2) where n = paths.length and m = maximum number parts in a given path and the
   * implementation is semi-optimized for performance.
   *
   * Note that this function only applies to absolute paths and does NOT dedupe paths, see the last example below.
   *
   * ['/a/b/c.js', '/a/d/c.js'] would return ['b/c.js', 'd/c.js']
   * ['/a/b/c.js', '/a/b/d.js'] would return ['c.js', 'd.js']
   * ['/a/b.js', '/c/a/b.js'] would return ['/a/b.js', 'c/a/b.js']
   * ['/a/b.js', '/a/b.js'] would return ['/a/b.js', '/a/b.js'] since this function does not dedupe.
   *
   * @param paths a list of paths to compute the minimal disambiguation
   * @param options.alwaysShowLeadingSeparator If true, all file paths will start with a leading
   * separator (`/` on Unix, `\` on Windows). If false, only full file paths will start with a leading
   * separator. Leading separators are never shown for paths that consist of only a filename, e.g.
   * "foo.txt". Defaults to true.
   * @param options.maxDepth maximum depth to truncate paths to, even if it doesn't fully disambiguate.

   */
export function minimalDisambiguousPaths(paths: string[], options: Options = {}): string[] {
  const pathObjects: PathObject[] = paths.map(path => {
    const separator = guessSeparator(path);
    const rootPrefixResult = /^(\w:).*/.exec(path);
    const rootPrefix = rootPrefixResult != null ? rootPrefixResult[1] : '';

    return {
      depth: null,
      parts: path
        .split(separator)
        // Path parts are reversed for easier processing below.
        .reverse()
        .filter(part => part !== '' && part !== rootPrefix),
      rootPrefix,
      separator,
      // paths which start with `/` or `\` or `C:\` should keep that prefix when they remain at the root level.
      hadLeadingSeparator:
        path.startsWith(separator) || (rootPrefix.length > 0 && path.startsWith(rootPrefix)),
    };
  });

  const maxDepth =
    options.maxDepth == null
      ? Math.max(...pathObjects.map(pathObject => pathObject.parts.length))
      : options.maxDepth;

  const pathObjectsToProcess = new Set(pathObjects);
  // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment -- Please fix as you edit this code
  const groupedPathObjects: Map<string, Set<PathObject>> = new Map();

  for (let currentDepth = 1; currentDepth <= maxDepth; currentDepth++) {
    // Group path objects by their path up to the current depth.
    groupedPathObjects.clear();
    for (const pathObject of pathObjectsToProcess) {
      const path = pathObject.parts.slice(0, currentDepth).join(pathObject.separator);
      if (!groupedPathObjects.has(path)) {
        groupedPathObjects.set(path, new Set());
      }
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- Please fix as you edit this code
      groupedPathObjects.get(path)!.add(pathObject);
    }

    // Mark the depth for unique path objects and remove them from the set of objects to process.
    for (const pathObjectGroup of groupedPathObjects.values()) {
      if (pathObjectGroup.size === 1) {
        const pathObject = Array.from(pathObjectGroup)[0];
        pathObject.depth = currentDepth;
        pathObjectsToProcess.delete(pathObject);
      }
    }
  }

  return pathObjects.map(({depth, parts, rootPrefix, separator, hadLeadingSeparator}) => {
    let resultPathParts = parts.slice(0, depth == null ? maxDepth : depth).reverse();

    // Empty path ('/' or 'c:\') should return a separator to indicate root.
    if (resultPathParts.length === 0) {
      return `${rootPrefix}${separator}`;
    }

    // - Complex paths should always start with a separator, if the original paths started with a separator
    // - Simple paths (e.g. single directory or file) should not start with a separator
    // - If we show the full path (no truncation), include any prefix like 'C:' as well.
    if (
      (resultPathParts.length === 1 && resultPathParts[0] === '') ||
      (resultPathParts.length > 1 && resultPathParts[0] !== '')
    ) {
      resultPathParts =
        resultPathParts.length === parts.length
          ? hadLeadingSeparator
            ? [rootPrefix, ...resultPathParts]
            : resultPathParts
          : (options.alwaysShowLeadingSeparator ?? hadLeadingSeparator)
            ? ['', ...resultPathParts]
            : resultPathParts;
    }

    return resultPathParts.join(separator);
  });
}

function guessSeparator(path: string): '/' | '\\' {
  const windowsCount = path.replace(/[^\\]/g, '').length;
  const unixCount = path.replace(/[^/]/g, '').length;

  return windowsCount > unixCount ? '\\' : '/';
}
