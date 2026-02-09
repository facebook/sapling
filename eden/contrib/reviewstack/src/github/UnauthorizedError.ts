/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Error thrown when a GitHub API request returns a 401 Unauthorized response.
 * This typically indicates that the access token has expired or been revoked.
 */
export default class UnauthorizedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'UnauthorizedError';
    // Maintains proper prototype chain for instanceof checks
    Object.setPrototypeOf(this, UnauthorizedError.prototype);
  }
}
