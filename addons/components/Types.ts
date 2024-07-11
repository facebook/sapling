/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type TypeaheadResult = {
  /** The display text of the suggestion */
  label: string;

  /**
   * Additional details to show de-emphasized next to the display name.
   * If provided, this is shown visually instead of the value.
   */
  detail?: string;

  /**
   * The literal value of the suggestion, placed literally as text into the commit message.
   * If `detail` is not provided, value is shown de-emphasized next to the display name.
   */
  value: string;

  /**
   * An optional image url representing this result. Usually, a user avatar.
   */
  image?: string;
};
