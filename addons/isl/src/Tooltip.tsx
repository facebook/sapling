/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MouseEvent, ReactNode} from 'react';
import type {ExclusiveOr} from 'shared/typeUtils';

import {findParentWithClassName} from './utils';
import React, {useEffect, useRef, useState} from 'react';
import ReactDOM from 'react-dom';
import {unwrap} from 'shared/utils';

import './Tooltip.css';

type Placement = 'top' | 'bottom' | 'left' | 'right';

/**
 * Default delay used for hover tooltips to convey documentation information.
 */
export const DOCUMENTATION_DELAY = 750;

/**
 * Enables child elements to render a tooltip when hovered/clicked.
 * Children are always rendered, but the tooltip is not rendered until triggered.
 * Tooltip is centered on bounding box of children.
 * You can adjust the trigger method:
 *  - 'hover' (default) to appear when mouse hovers container element
 *  - 'click' to appear when mouse clicks container element
 *  - 'manual' to control programmatically by providing `shouldShow` prop.
 *  - 'disabled' to turn off hover/click support programmatically
 *
 * You can adjust which side the tooltip appears on.
 *   Default placement is 'top', above the element.
 *
 * Tooltip content may either be a (i18n-ized) string `title`, or a `Component` to render.
 *   `title`s will automatically wrap text,
 *   but `Component`s are expected to handle their own sizing.
 *
 * Tooltips will hide themselves when you scroll or resize.
 * This applies even to manual tooltips with shouldShow=true.
 */
export function Tooltip({
  children,
  title,
  component: Component,
  placement: placementProp,
  trigger: triggerProp,
  delayMs,
  shouldShow,
}: {
  children: ReactNode;
  placement?: Placement;
  /**
   * Applies delay to visual appearance of tooltip.
   * Note element is always constructed immediately.
   * This delay applies to all trigger methods, even 'manual'.
   * The delay is only on the leading-edge; disappearing is always instant.
   */
  delayMs?: number;
} & ExclusiveOr<
  {trigger: 'manual'; shouldShow: boolean},
  {trigger?: 'hover' | 'click' | 'disabled'}
> &
  ExclusiveOr<{component: () => JSX.Element}, {title: string}>) {
  const trigger = triggerProp ?? 'hover';
  const placement = placementProp ?? 'top';
  const [visible, setVisible] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const content = Component == null ? title : <Component />;
  useEffect(() => {
    if (typeof shouldShow === 'boolean') {
      setVisible(shouldShow);
    }
  }, [shouldShow]);

  useEffect(() => {
    if (trigger === 'click') {
      if (visible) {
        // When using click trigger, we need to listen for clicks outside the tooltip
        // to close it again.
        const globalClickHandler = (e: Event) => {
          if (!eventIsFromInsideTooltip(e as unknown as MouseEvent)) {
            setVisible(false);
          }
        };
        window.addEventListener('click', globalClickHandler);
        return () => window.removeEventListener('click', globalClickHandler);
      }
    }
  }, [visible, setVisible, trigger]);

  // scrolling or resizing the window should hide all tooltips to prevent lingering.
  useEffect(() => {
    if (visible) {
      const hideTooltip = (e: Event) => {
        if (e.type === 'keyup') {
          if ((e as KeyboardEvent).key === 'Escape') {
            setVisible(false);
          }
        } else if (e.type === 'resize' || !eventIsFromInsideTooltip(e as unknown as MouseEvent)) {
          setVisible(false);
        }
      };
      window.addEventListener('scroll', hideTooltip, true);
      window.addEventListener('resize', hideTooltip, true);
      window.addEventListener('keyup', hideTooltip, true);
      return () => {
        window.removeEventListener('scroll', hideTooltip, true);
        window.removeEventListener('resize', hideTooltip, true);
        window.removeEventListener('keyup', hideTooltip, true);
      };
    }
  }, [visible, setVisible]);

  return (
    <div
      className="tooltip-creator"
      ref={ref}
      onClick={
        trigger === 'click'
          ? (event: MouseEvent) => {
              if (!visible || !eventIsFromInsideTooltip(event)) {
                setVisible(val => !val);
                // don't trigger global click listener in the same tick
                event.stopPropagation();
              }
            }
          : undefined
      }
      onMouseEnter={trigger === 'hover' ? () => setVisible(true) : undefined}
      onMouseLeave={trigger === 'hover' ? () => setVisible(false) : undefined}>
      {visible && ref.current && (
        <RenderTooltipOnto delayMs={delayMs} element={ref.current} placement={placement}>
          {content}
        </RenderTooltipOnto>
      )}
      {children}
    </div>
  );
}

/**
 * If you click inside a tooltip triggered by click, we don't want to dismiss the tooltip.
 * We consider any click in a descendant of ANY tooltip as a click.
 * Same applies for scroll events inside tooltips.
 */
function eventIsFromInsideTooltip(event: MouseEvent): boolean {
  const parentTooltip = findParentWithClassName(event.target as HTMLElement, 'tooltip');
  return parentTooltip != null;
}

function RenderTooltipOnto({
  element,
  placement,
  children,
  delayMs,
}: {
  element: HTMLElement;
  placement: Placement;
  children: ReactNode;
  delayMs?: number;
}) {
  const sourceBoundingRect = element.getBoundingClientRect();
  const tooltipRef = useRef<HTMLDivElement | null>(null);

  let effectivePlacement = placement;

  // to center the tooltip over the tooltip-creator, we need to measure its final rendered size
  const renderedDimensions = useRenderedDimensions(tooltipRef);
  const position = offsetsForPlacement(placement, sourceBoundingRect, renderedDimensions);
  effectivePlacement = position.autoPlacement ?? placement;
  // The tooltip may end up overflowing off the screen, since it's rendered absolutely.
  // We can push it back as needed with an additional offet.
  const viewportAdjust = getViewportAdjustedDelta(effectivePlacement, position, renderedDimensions);

  const viewportDimensions = document.body.getBoundingClientRect();
  const style: React.CSSProperties = {
    animationDelay: delayMs ? `${delayMs}ms` : undefined,
  };

  if (position.left > viewportDimensions.width / 2) {
    // All our position computations use top+left.
    // If we position using `left`, but the tooltip is near the right edge,
    // it will squish itself to fit rather than push itself further left.
    // Instead, we need to position with `right`, computed from left. based on the viewport dimension.
    style.right =
      viewportDimensions.width - (position.left + viewportAdjust.left + renderedDimensions.width);
  } else {
    style.left = position.left + viewportAdjust.left;
  }
  // Note: The same could technically apply for top/bottom, but only for left/right positioned tooltips which are less common,
  // so in practice it matters less.
  if (position.top > viewportDimensions.height / 2) {
    style.bottom =
      viewportDimensions.height - (position.top + viewportAdjust.top + renderedDimensions.height);
  } else {
    style.top = position.top + viewportAdjust.top;
  }

  // Use a portal so the tooltip element is rendered into the global list of tooltips,
  // rather than as a descendant of the tooltip creator.
  // This allows us to use absolute coordinates for positioning, and for
  // tooltips to "escape" their containing elements, scroll, inherited styles, etc.
  return ReactDOM.createPortal(
    <div
      ref={tooltipRef}
      role="tooltip"
      className={
        `tooltip tooltip-${effectivePlacement}` +
        (typeof children === 'string' ? ' simple-text-tooltip' : '')
      }
      style={style}>
      <div
        className={`tooltip-arrow tooltip-arrow-${effectivePlacement}`}
        // If we had to push the tooltip back to prevent overflow,
        // we also need to move the arrow the opposite direction so it still lines up.
        style={{transform: `translate(${-viewportAdjust.left}px, ${-viewportAdjust.top}px)`}}
      />
      {children}
    </div>,
    findParentTooltipContainer(element),
  );
}

let foundRoot: HTMLElement;
const findParentTooltipContainer = (element: HTMLElement): HTMLElement => {
  if (foundRoot) {
    // memoize since our root component won't change
    return foundRoot;
  }
  const islRoot = unwrap(findParentWithClassName(element, 'isl-root'));
  foundRoot = unwrap(islRoot.querySelector('.tooltip-root-container')) as HTMLElement;
  return foundRoot;
};

export const __TEST__ = {
  resetMemoizedTooltipContainer: () => {
    // memoizing breaks across multiple test cases, we need to reset it after each test
    foundRoot = undefined as unknown as HTMLElement;
  },
};

/**
 * Offset tooltip from tooltipCreator's absolute position using `placement`,
 * such that it is centered and on the correct side.
 * This requires the rendered tooltip's width and height (for centering).
 * Coordinates are left&top absolute offsets.
 *
 * When appropriate, we also detect if this placement would go offscreen
 * and instead provide a better placement.
 *
 * In this diagram, `0` is `tooltipCreatorRect`,
 * `1` is what we want to compute using 0 and the size of the rendered tooltip.
 *
 *     0---*
 *     |   |     <- tooltip creator (The thing you hover to see the tooltip)
 *     *---*
 *       ^       <- tooltip arrow
 *  1---------+
 *  |         |  <- tooltip
 *  +---------+
 *
 */
function offsetsForPlacement(
  placement: Placement,
  tooltipCreatorRect: DOMRect,
  tooltipDimensions: {width: number; height: number},
): {top: number; left: number; autoPlacement?: Placement} {
  const padding = 5;
  switch (placement) {
    case 'top': {
      const top = tooltipCreatorRect.top - padding - tooltipDimensions.height;
      const left =
        tooltipCreatorRect.left + tooltipCreatorRect.width / 2 - tooltipDimensions.width / 2;
      if (top < 0) {
        // Placing the tooltip here would overflow off the top of the page,
        // let's instead place it on the bottom.
        // TODO: we could do this for the other directions too.
        return {
          top: tooltipCreatorRect.top + tooltipCreatorRect.height + padding,
          left,
          autoPlacement: 'bottom',
        };
      }
      return {
        top,
        left,
      };
    }
    case 'bottom': {
      return {
        top: tooltipCreatorRect.top + tooltipCreatorRect.height + padding,
        left: tooltipCreatorRect.left + tooltipCreatorRect.width / 2 - tooltipDimensions.width / 2,
      };
    }
    case 'left': {
      return {
        top: tooltipCreatorRect.top + tooltipCreatorRect.height / 2 - tooltipDimensions.height / 2,
        left: tooltipCreatorRect.left - tooltipDimensions.width - padding,
      };
    }
    case 'right': {
      return {
        top: tooltipCreatorRect.top + tooltipCreatorRect.height / 2 - tooltipDimensions.height / 2,
        left: tooltipCreatorRect.right + padding,
      };
    }
  }
}

/**
 * If the rendered tooltip would overflow outside the screen bounds,
 * we need to translate the tooltip back into bounds.
 */
function getViewportAdjustedDelta(
  placement: Placement,
  pos: {top: number; left: number},
  renderedDimensions: {width: number; height: number},
): {left: number; top: number} {
  const delta = {top: 0, left: 0};

  const viewportPadding = 5;
  const viewportDimensions = document.body.getBoundingClientRect();

  if (placement === 'right' || placement === 'left') {
    const topEdgeOffset = pos.top - viewportPadding;
    const bottomEdgeOffset = pos.top + viewportPadding + renderedDimensions.height;
    if (topEdgeOffset < viewportDimensions.top) {
      // top overflow
      delta.top = viewportDimensions.top - topEdgeOffset;
    } else if (bottomEdgeOffset > viewportDimensions.top + viewportDimensions.height) {
      // bottom overflow
      delta.top = viewportDimensions.top + viewportDimensions.height - bottomEdgeOffset;
    }
  } else {
    const leftEdgeOffset = pos.left - viewportPadding;
    const rightEdgeOffset = pos.left + viewportPadding + renderedDimensions.width;
    if (leftEdgeOffset < viewportDimensions.left) {
      // left overflow
      delta.left = viewportDimensions.left - leftEdgeOffset;
    } else if (rightEdgeOffset > viewportDimensions.right) {
      // right overflow
      delta.left = viewportDimensions.left + viewportDimensions.width - rightEdgeOffset;
    }
  }

  return delta;
}

function useRenderedDimensions(ref: React.MutableRefObject<HTMLDivElement | null>) {
  const [dimensions, setDimensions] = useState({width: 0, height: 0});

  useEffect(() => {
    if (ref.current) {
      setDimensions({
        width: ref.current.offsetWidth,
        height: ref.current.offsetHeight,
      });
    }
  }, [ref]);

  return dimensions;
}
