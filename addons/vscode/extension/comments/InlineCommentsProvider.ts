/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {VSCodeReposList} from '../VSCodeRepo';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {Disposable} from 'vscode';

export class InlineCommentsProvider implements Disposable {
  constructor(private reposList: VSCodeReposList, private context: RepositoryContext) {}
  dispose(): void {}
}
