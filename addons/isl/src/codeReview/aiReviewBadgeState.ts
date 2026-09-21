/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {configBackedAtom} from '../jotaiUtils';

// A leaf module rather than SettingsTooltip, which the badge cannot import:
// SettingsTooltip -> Internal -> InternalImports -> phabricator.tsx is a cycle.
export const showAiReviewingBadge = configBackedAtom<boolean>('isl.show-ai-reviewing-badge', true);
