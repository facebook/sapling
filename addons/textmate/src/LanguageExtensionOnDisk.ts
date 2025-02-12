/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {promises as fs} from 'node:fs';
import pathMod from 'node:path';
import AbstractLanguageExtension from './AbstractLanguageExtension.js';

export default class LanguageExtensionOnDisk extends AbstractLanguageExtension {
  constructor(private extensionRoot: string) {
    super();
  }

  getContents(pathRelativeToExtensionRoot: string): Promise<string> {
    const fullPath = pathMod.join(this.extensionRoot, pathRelativeToExtensionRoot);
    return fs.readFile(fullPath, {encoding: 'utf8'});
  }

  toString(): string {
    return this.extensionRoot;
  }
}
