/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const COMPONENT_PADDING = 10;
export const BranchIndicator = () => {
  const width = COMPONENT_PADDING * 2;
  const height = COMPONENT_PADDING * 3;
  // Compensate for line width
  const startX = width + 1;
  const startY = 0;
  const endX = 0;
  const endY = height;
  const verticalLead = height * 0.75;
  const path =
    // start point
    `M${startX} ${startY}` +
    // cubic bezier curve to end point
    `C ${startX} ${startY + verticalLead}, ${endX} ${endY - verticalLead}, ${endX} ${endY}`;
  return (
    <svg
      className="branch-indicator"
      width={width + 2 /* avoid border clipping */}
      height={height}
      xmlns="http://www.w3.org/2000/svg">
      <path d={path} strokeWidth="2px" fill="transparent" />
    </svg>
  );
};
