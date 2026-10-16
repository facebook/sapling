/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Appends the instructions the user typed into the smart actions context input. */
export function withUserContext<T extends {modelPrompt: string}>(
  prompt: T,
  userContext: string | undefined,
): T {
  return userContext
    ? {
        ...prompt,
        modelPrompt: `${prompt.modelPrompt}\n\nIMPORTANT - Additional Instructions from the User: ${userContext}`,
      }
    : prompt;
}
