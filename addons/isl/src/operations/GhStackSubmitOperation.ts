/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Operation} from './Operation';

export class GhStackSubmitOperation extends Operation {
  static opName = 'ghstack submit';

  constructor(private options?: {draft?: boolean; updateMessage?: string}) {
    super('GhStackSubmitOperation');
  }

  getArgs() {
    const args = ['ghstack', 'submit'];
    if (this.options?.draft) {
      args.push('--draft');
    }
    if (this.options?.updateMessage) {
      args.push('--message', this.options?.updateMessage);
    }
    return args;
  }
}
