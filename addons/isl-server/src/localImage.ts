/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const IMAGE_MIME_TYPES: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
};

const MAX_IMAGE_BYTES = 10 * 1024 * 1024;

/**
 * Directories a relative image path in a commit message may resolve against: the repo root,
 * then any listed in the comma-separated `isl.image-roots` config, which may start with `~`.
 */
export function localImageRoots(repoRoot: string, configuredRoots: string | undefined): string[] {
  const extra = (configuredRoots ?? '')
    .split(',')
    .map(root => root.trim())
    .filter(root => root !== '')
    .map(root => (root === '~' || root.startsWith('~/') ? os.homedir() + root.slice(1) : root))
    .map(root => path.resolve(repoRoot, root));
  return [repoRoot, ...extra];
}

function isInside(root: string, target: string): boolean {
  const relative = path.relative(root, target);
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
}

/** The real path of `relativePath` under `root`, or null when it is missing or leaves the root. */
async function resolveInRoot(root: string, relativePath: string): Promise<string | null> {
  const candidate = path.resolve(root, relativePath);
  if (!isInside(root, candidate)) {
    return null;
  }
  try {
    const [realRoot, realPath] = await Promise.all([
      fs.promises.realpath(root),
      fs.promises.realpath(candidate),
    ]);
    // Symlinks may point anywhere, so check the resolved path too.
    return isInside(realRoot, realPath) ? realPath : null;
  } catch {
    return null;
  }
}

/**
 * Read an image referenced by a relative path as a `data:` URL, from the first root that holds it.
 * Only image files inside a root are read, so a path cannot reach the rest of the disk.
 */
export async function readLocalImage(roots: Array<string>, src: string): Promise<string> {
  const relativePath = decodeURIComponent(src);
  if (path.isAbsolute(relativePath)) {
    throw new Error(`Image path must be relative: ${src}`);
  }
  const mimeType = IMAGE_MIME_TYPES[path.extname(relativePath).toLowerCase()];
  if (mimeType == null) {
    throw new Error(`Not an image file: ${src}`);
  }
  const resolved = await Promise.all(roots.map(root => resolveInRoot(root, relativePath)));
  const realPath = resolved.find(candidate => candidate != null);
  if (realPath == null) {
    throw new Error(`Image not found under ${roots.join(', ')}: ${src}`);
  }
  const {size} = await fs.promises.stat(realPath);
  if (size > MAX_IMAGE_BYTES) {
    throw new Error(`Image is larger than ${MAX_IMAGE_BYTES} bytes: ${src}`);
  }
  const content = await fs.promises.readFile(realPath);
  return `data:${mimeType};base64,${content.toString('base64')}`;
}
