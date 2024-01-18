/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {prefersReducedMotion} from './mediaQuery';
import deepEqual from 'fast-deep-equal';
import React, {useRef, useLayoutEffect} from 'react';

type ReorderGroupProps = React.HTMLAttributes<HTMLDivElement> & {
  children: React.ReactElement[];
  animationDuration?: number;
  animationMinPixel?: number;
};

type PreviousState = {
  // Ordered list of `data-reorder-id`s. Can ONLY be updated inside `useLayoutEffect`.
  // Useful to test if `children` has changed or not.
  idList: Array<string>;

  // Locations of the old children, keyed by `data-reorder-id`.
  rectMap: Map<string, DOMRect>;
};

const emptyPreviousState: Readonly<PreviousState> = {
  idList: [],
  rectMap: new Map(),
};

/**
 * AnimatedReorderGroup tracks and animates elements with the `data-reorder-id` attribute.
 * Elements with the same `data-reorder-id` will be animated on position change.
 *
 * Beware that while `data-reorder-id` can be put on nested elements, animation is
 * only triggered when the `children` of this component is changed.
 *
 * This component only handles reordering, if you want drag and drop support or animations
 * on inserted or deleted items, you might want to use other components together.
 */
export const AnimatedReorderGroup: React.FC<ReorderGroupProps> = ({
  children,
  animationDuration,
  animationMinPixel,
  ...props
}) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const previousStateRef = useRef<Readonly<PreviousState>>(emptyPreviousState);

  useLayoutEffect(() => {
    const animate = !prefersReducedMotion();
    updatePreviousState(
      containerRef,
      previousStateRef,
      animate,
      animationDuration,
      animationMinPixel,
    );
  }, [children, animationDuration, animationMinPixel]);

  // Try to get the rects of old children right before rendering new children
  // and calling the LayoutEffect callback. This captures position changes
  // since the last useLayoutEffect. The position changes might be caused by
  // scrolling or resizing the window.
  updatePreviousState(containerRef, previousStateRef, false, animationDuration);

  return (
    <div {...props} ref={containerRef}>
      {children}
    </div>
  );
};

function scanElements(containerRef: React.RefObject<HTMLDivElement | null>): HTMLElement[] {
  const container = containerRef.current;
  if (container == null) {
    return [];
  }
  const elements = container.querySelectorAll<HTMLElement>('[data-reorder-id]');
  return [...elements];
}

function updatePreviousState(
  containerRef: React.RefObject<HTMLDivElement>,
  previousStateRef: React.MutableRefObject<Readonly<PreviousState>>,
  animate = false,
  animationDuration = 200,
  animationMinPixel = 5,
) {
  const elements = scanElements(containerRef);
  const idList: Array<string> = [];
  const rectMap = new Map<string, DOMRect>();
  elements.forEach(element => {
    const reorderId = element.getAttribute('data-reorder-id');
    if (reorderId == null || reorderId === '') {
      return;
    }
    idList.push(reorderId);
    const newBox = element.getBoundingClientRect();
    if (animate) {
      const oldBox = previousStateRef.current.rectMap.get(reorderId);
      if (oldBox && (oldBox.x !== newBox.x || oldBox.y !== newBox.y)) {
        // Animate from old to the new (current) rect.
        const dx = oldBox.left - newBox.left;
        const dy = oldBox.top - newBox.top;
        if (Math.abs(dx) + Math.abs(dy) > animationMinPixel) {
          element.animate(
            [{transform: `translate(${dx}px,${dy}px)`}, {transform: 'translate(0,0)'}],
            {duration: animationDuration, easing: 'ease-out'},
          );
        }
      }
    }
    rectMap.set(reorderId, newBox);
  });

  if (!animate && !deepEqual(idList, previousStateRef.current.idList)) {
    // If animate is false, we want to get the rects of the old children.
    // If the idList mismatches, it's not the "old" children so we discard
    // the result.
    return;
  }

  previousStateRef.current = {
    idList,
    rectMap,
  };
}
