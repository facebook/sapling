/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type VSCodeAPI = {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  postMessage(message: any): void;
};

declare global {
  function acquireVsCodeApi(): VSCodeAPI;
}

export const vscodeApi = acquireVsCodeApi();
