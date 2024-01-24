/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './IconStack.css';

/**
 * Render two Icons on top of each other, the top one half-sized in the bottom right corner.
 */
export function IconStack({
  children,
  ...rest
}: {children: [React.ReactNode, React.ReactNode]} & React.DetailedHTMLProps<
  React.HTMLAttributes<HTMLDivElement>,
  HTMLDivElement
>) {
  return (
    <div className="icon-stack" {...rest}>
      {children}
    </div>
  );
}
