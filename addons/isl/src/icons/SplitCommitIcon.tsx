/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export function SplitCommitIcon(props: {slot?: 'start'}) {
  return (
    <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg" {...props}>
      <g stroke="currentColor" strokeWidth={4}>
        <path d="M 51.3384,38.090562 16.404011,45.516095 A 18,18 0 0 0 37.197792,57.45369 18,18 0 0 0 51.3384,38.090562 Z" />
        <path d="M 26.802208,8.54631 A 18,18 0 0 0 12.650137,27.911875 L 47.607452,20.481468 A 18,18 0 0 0 26.802208,8.54631 Z" />
        <path
          strokeDasharray="17.6,4"
          strokeWidth={6}
          d="M 2.6555718,39.237351 61.344428,26.762649"
        />
      </g>{' '}
    </svg>
  );
}
