/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

type Navigate = (to: string) => void;

type UseNavigate = () => Navigate;

let navigateHook: UseNavigate = () => to => {
  window.location.href = to;
};

export function setCustomNavigateHook(customNavigateHook: UseNavigate): void {
  navigateHook = customNavigateHook;
}

export default function useNavigate(): Navigate {
  return navigateHook();
}
