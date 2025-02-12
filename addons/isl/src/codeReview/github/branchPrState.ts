/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {codeReviewProvider} from '../CodeReviewInfo';

/** Experimental backdoor setting to get around disabled submit modes. Useful for testing. */
export const overrideDisabledSubmitModes = atom(process.env.NODE_ENV === 'development');

export const experimentalBranchPRsEnabled = atom(process.env.NODE_ENV === 'development');

export const branchPRsSupported = atom(get => {
  const supported = get(codeReviewProvider)?.supportBranchingPrs ?? false;
  const enabled = get(experimentalBranchPRsEnabled);
  return supported && enabled;
});
