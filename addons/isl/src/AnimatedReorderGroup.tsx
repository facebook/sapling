/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React, {useLayoutEffect, useRef} from 'react';
import {prefersReducedMotion} from './mediaQuery';

type ReorderGroupProps = React.HTMLAttributes<HTMLDivElement> & {
  children: React.ReactElement[];
  animationDuration?: number;
  animationMinPixel?: number;
  disableAnimation?: boolean;
};

type ElementPosition = {left: number; top: number};
type ElementMovement = [HTMLElement, number, number];

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
  disableAnimation,
  ...props
}) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const animationControllerRef = useRef<ReorderAnimationController | null>(null);
  if (animationControllerRef.current == null) {
    animationControllerRef.current = new ReorderAnimationController();
  }
  const animationController = animationControllerRef.current;
  const reducedMotion = prefersReducedMotion();

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (container == null || reducedMotion || disableAnimation) {
      animationController.reset();
      return;
    }
    animationController.update(container, animationMinPixel, animationDuration);
  }, [
    animationController,
    children,
    animationDuration,
    animationMinPixel,
    reducedMotion,
    disableAnimation,
  ]);

  useLayoutEffect(() => () => animationController.dispose(), [animationController]);

  return (
    <div {...props} ref={containerRef}>
      {children}
    </div>
  );
};

class ReorderAnimationController {
  private animationFrame: {id: number; view: Window} | null = null;
  private animations = new Set<Animation>();
  private previousPositions = new Map<string, ElementPosition>();

  update(container: HTMLElement, animationMinPixel = 5, animationDuration = 200) {
    this.cancelAnimations();
    const movements = this.measureMovements(container, animationMinPixel);
    const view = container.ownerDocument.defaultView;
    if (view == null || movements.length === 0) {
      return;
    }
    const frameId = view.requestAnimationFrame(() => {
      if (this.animationFrame?.id !== frameId) {
        return;
      }
      this.animationFrame = null;
      for (const [element, dx, dy] of movements) {
        const animation = element.animate(
          [{transform: `translate(${dx}px,${dy}px)`}, {transform: 'translate(0,0)'}],
          {duration: animationDuration, easing: 'ease-out'},
        );
        this.animations.add(animation);
        animation.onfinish = () => this.animations.delete(animation);
      }
    });
    this.animationFrame = {id: frameId, view};
  }

  reset() {
    this.cancelAnimations();
    this.previousPositions = new Map();
  }

  dispose() {
    this.reset();
  }

  private measureMovements(container: HTMLElement, animationMinPixel: number): ElementMovement[] {
    const containerBox = container.getBoundingClientRect();
    const nextPositions = new Map<string, ElementPosition>();
    const movements: ElementMovement[] = [];
    for (const element of container.querySelectorAll<HTMLElement>('[data-reorder-id]')) {
      const reorderId = element.getAttribute('data-reorder-id');
      if (reorderId == null || reorderId === '') {
        continue;
      }
      const box = element.getBoundingClientRect();
      const position = {
        left: box.left - containerBox.left,
        top: box.top - containerBox.top,
      };
      const previous = this.previousPositions.get(reorderId);
      if (previous != null) {
        const dx = previous.left - position.left;
        const dy = previous.top - position.top;
        if (Math.abs(dx) + Math.abs(dy) > animationMinPixel) {
          movements.push([element, dx, dy]);
        }
      }
      nextPositions.set(reorderId, position);
    }
    this.previousPositions = nextPositions;
    return movements;
  }

  private cancelAnimations() {
    if (this.animationFrame != null) {
      this.animationFrame.view.cancelAnimationFrame(this.animationFrame.id);
      this.animationFrame = null;
    }
    for (const animation of this.animations) {
      animation.cancel();
    }
    this.animations.clear();
  }
}
