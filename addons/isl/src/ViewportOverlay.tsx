/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import React, {useEffect, useRef} from 'react';
import ReactDOM from 'react-dom';

const styles = stylex.create({
  root: {
    position: 'absolute',
    width: '100vw',
    height: '100vh',
    pointerEvents: 'none',
    zIndex: 1000,
  },
});

/**
 * Render `children` as an overlay, in a container that uses absolute positioning.
 * Suitable for tooltips, menus, and dragging elements.
 */
export function ViewportOverlay(props: {
  children: React.ReactNode;
  key?: React.Key | null;
}): React.ReactPortal {
  const {key, children} = props;
  return ReactDOM.createPortal(
    children as Parameters<
      typeof ReactDOM.createPortal
    >[0] /** ReactDOM's understanding of ReactNode seems wrong here */,
    getRootContainer(),
    key == null ? null : `overlay-${key}`,
  ) as React.ReactPortal;
}

let cachedRoot: HTMLElement | undefined;
const getRootContainer = (): HTMLElement => {
  if (cachedRoot) {
    // memoize since our root component won't change
    return cachedRoot;
  }
  throw new Error(
    'ViewportOverlayRoot not found. Make sure you render it at the root of the tree.',
  );
};

export function ViewportOverlayRoot() {
  const rootRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (rootRef.current) {
      cachedRoot = rootRef.current;
    }
    return () => {
      cachedRoot = undefined;
    };
  }, []);
  return <div ref={rootRef} {...stylex.props(styles.root)} data-testid="viewport-overlay-root" />;
}
