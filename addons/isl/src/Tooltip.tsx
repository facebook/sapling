/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MouseEvent, ReactNode} from 'react';
import type {TypedEventEmitter} from 'shared/TypedEventEmitter';
import type {ExclusiveOr} from 'shared/typeUtils';

import React, {useLayoutEffect, useEffect, useRef, useState} from 'react';
import ReactDOM from 'react-dom';
import {findParentWithClassName} from 'shared/utils';
import {getZoomLevel} from 'shared/zoom';

import './Tooltip.css';

export type Placement = 'top' | 'bottom' | 'left' | 'right';

/**
 * Default delay used for hover tooltips to convey documentation information.
 */
export const DOCUMENTATION_DELAY = 750;

type TooltipProps = {
  children: ReactNode;
  placement?: Placement;
  /**
   * Applies delay to visual appearance of tooltip.
   * Note element is always constructed immediately.
   * This delay applies to all trigger methods except 'click'.
   * The delay is only on the leading-edge; disappearing is always instant.
   */
  delayMs?: number;
  /**
   * Callback to run when the tooltip is dismissed for any reason.
   * For 'click' tooltips that also have a 'title', this only fires when the 'click' tooltip is dismissed.
   * Note: `onDismiss` will not run if the entire <Tooltip> is unmounted while the tooltip is visible.
   */
  onDismiss?: () => unknown;
} & ExclusiveOr<
  ExclusiveOr<{trigger: 'manual'; shouldShow: boolean}, {trigger?: 'hover' | 'disabled'}> &
    ExclusiveOr<
      {component: (dismiss: () => void) => JSX.Element},
      {title: string | React.ReactNode}
    >,
  {
    trigger: 'click';
    component: (dismiss: () => void) => JSX.Element;
    title?: string | React.ReactNode;
    additionalToggles?: TypedEventEmitter<'change', unknown>;
  }
>;

type VisibleState =
  | true /* primary content (prefers component) is visible */
  | false
  | 'title' /* 'title', not 'component' is visible */;

/**
 * Enables child elements to render a tooltip when hovered/clicked.
 * Children are always rendered, but the tooltip is not rendered until triggered.
 * Tooltip is centered on bounding box of children.
 * You can adjust the trigger method:
 *  - 'hover' (default) to appear when mouse hovers container element
 *  - 'click' to render `component` on click, render `title` on hover.
 *  - 'manual' to control programmatically by providing `shouldShow` prop.
 *  - 'disabled' to turn off hover/click support programmatically
 *
 * You can adjust which side the tooltip appears on.
 *   Default placement is 'top', above the element.
 *
 * Tooltip content may either be a (i18n-ized) string `title`, or a `Component` to render.
 *   `title`s will automatically wrap text,
 *   but `Component`s are expected to handle their own sizing.
 * `Component`-rendered content allows pointer events inside the tooltip,
 *   but string `title`s do not allow pointer events, and dismiss if the mouse exits
 *   the original tooltip creator.
 *
 * Tooltips will hide themselves when you scroll or resize.
 * This applies even to manual tooltips with shouldShow=true.
 */
export function Tooltip({
  children,
  title,
  component,
  placement: placementProp,
  trigger: triggerProp,
  delayMs,
  shouldShow,
  onDismiss,
  additionalToggles,
}: TooltipProps) {
  const trigger = triggerProp ?? 'hover';
  const placement = placementProp ?? 'top';
  const [visible, setVisible] = useState<VisibleState>(false);

  // trigger onDismiss when visibility newly becomes false
  const lastVisible = useRef(false);
  useEffect(() => {
    if (!visible && lastVisible.current === true) {
      onDismiss?.();
    }
    lastVisible.current = visible === true;
  }, [visible, onDismiss, lastVisible]);

  const ref = useRef<HTMLDivElement>(null);
  const getContent = () => {
    if (visible === 'title') {
      return title;
    }
    return component == null ? title : component(() => setVisible(false));
  };

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

  useEffect(() => {
    const cb = () => setVisible(last => !last);
    additionalToggles?.addListener('change', cb);
    return () => {
      additionalToggles?.removeListener('change', cb);
    };
  }, [additionalToggles]);

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

  // Using onMouseLeave directly on the div is unreliable if the component rerenders: https://github.com/facebook/react/issues/4492
  // Use a manually managed subscription instead.
  useLayoutEffect(() => {
    const needHover = trigger === 'hover' || (trigger === 'click' && title != null);
    if (!needHover) {
      return;
    }
    // Do not change visible if 'click' shows the content.
    const onMouseEnter = () =>
      setVisible(vis => (trigger === 'click' ? (vis === true ? vis : 'title') : true));
    const onMouseLeave = () =>
      setVisible(vis => (trigger === 'click' && vis === true ? vis : false));
    const div = ref.current;
    div?.addEventListener('mouseenter', onMouseEnter);
    div?.addEventListener('mouseleave', onMouseLeave);
    return () => {
      div?.removeEventListener('mouseenter', onMouseEnter);
      div?.removeEventListener('mouseleave', onMouseLeave);
    };
  }, [trigger, title]);

  // Force delayMs to be 0 when `component` is shown by click.
  const realDelayMs = trigger === 'click' && visible === true ? 0 : delayMs;

  return (
    <div
      className="tooltip-creator"
      ref={ref}
      onClick={
        trigger === 'click'
          ? (event: MouseEvent) => {
              if (visible !== true || !eventIsFromInsideTooltip(event)) {
                setVisible(vis => vis !== true);
                // don't trigger global click listener in the same tick
                event.stopPropagation();
              }
            }
          : undefined
      }>
      {visible && ref.current && (
        <RenderTooltipOnto delayMs={realDelayMs} element={ref.current} placement={placement}>
          {getContent()}
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

  const zoom = getZoomLevel();
  let effectivePlacement = placement;
  const viewportDimensions = document.body.getBoundingClientRect();
  viewportDimensions.width /= zoom;
  viewportDimensions.height /= zoom;

  // to center the tooltip over the tooltip-creator, we need to measure its final rendered size
  const renderedDimensions = useRenderedDimensions(tooltipRef, children);
  const position = offsetsForPlacement(
    placement,
    sourceBoundingRect,
    renderedDimensions,
    viewportDimensions,
  );
  effectivePlacement = position.autoPlacement ?? placement;
  // The tooltip may end up overflowing off the screen, since it's rendered absolutely.
  // We can push it back as needed with an additional offet.
  const viewportAdjust = getViewportAdjustedDelta(effectivePlacement, position, renderedDimensions);

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
    getTooltipContainer(),
  );
}

let cachedRoot: HTMLElement | undefined;
const getTooltipContainer = (): HTMLElement => {
  if (cachedRoot) {
    // memoize since our root component won't change
    return cachedRoot;
  }
  throw new Error(
    'TooltipRootContainer not found. Make sure you render it at the root of the tree.',
  );
};

export function TooltipRootContainer() {
  const rootRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (rootRef.current) {
      cachedRoot = rootRef.current;
    }
    return () => {
      cachedRoot = undefined;
    };
  }, []);
  return (
    <div ref={rootRef} className="tooltip-root-container" data-testid="tooltip-root-container" />
  );
}

type OffsetPlacement = {
  top: number;
  left: number;
  autoPlacement?: Placement;
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
  viewportDimensions: DOMRect,
): OffsetPlacement {
  const padding = 5;
  let result: OffsetPlacement = {top: 0, left: 0};
  let currentPlacement = placement;
  for (let i = 0; i <= 2; i++) {
    switch (currentPlacement) {
      case 'top': {
        result = {
          top: tooltipCreatorRect.top - padding - tooltipDimensions.height,
          left:
            tooltipCreatorRect.left + tooltipCreatorRect.width / 2 - tooltipDimensions.width / 2,
        };
        if (result.top < 0) {
          currentPlacement = 'bottom';
          continue;
        }
        break;
      }
      case 'bottom': {
        result = {
          top: tooltipCreatorRect.top + tooltipCreatorRect.height + padding,
          left:
            tooltipCreatorRect.left + tooltipCreatorRect.width / 2 - tooltipDimensions.width / 2,
        };
        if (result.top + tooltipDimensions.height > viewportDimensions.height) {
          currentPlacement = 'top';
          continue;
        }
        break;
      }
      case 'left': {
        result = {
          top:
            tooltipCreatorRect.top + tooltipCreatorRect.height / 2 - tooltipDimensions.height / 2,
          left: tooltipCreatorRect.left - tooltipDimensions.width - padding,
        };
        if (result.left < 0) {
          currentPlacement = 'right';
          continue;
        }
        break;
      }
      case 'right': {
        result = {
          top:
            tooltipCreatorRect.top + tooltipCreatorRect.height / 2 - tooltipDimensions.height / 2,
          left: tooltipCreatorRect.right + padding,
        };
        if (result.left + tooltipDimensions.width > viewportDimensions.width) {
          currentPlacement = 'left';
          continue;
        }
        break;
      }
    }
    break;
  }
  // Set autoPlacement if we chose a different placement.
  if (currentPlacement !== placement) {
    result.autoPlacement = currentPlacement;
  }
  return result;
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

  const zoom = getZoomLevel();
  viewportDimensions.width /= zoom;
  viewportDimensions.height /= zoom;

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

function useRenderedDimensions(ref: React.MutableRefObject<HTMLDivElement | null>, deps: unknown) {
  const [dimensions, setDimensions] = useState({width: 0, height: 0});

  useLayoutEffect(() => {
    const target = ref.current;
    if (target == null) {
      return;
    }

    const updateDimensions = () => {
      setDimensions({
        width: target.offsetWidth,
        height: target.offsetHeight,
      });
    };

    updateDimensions();

    const observer = new ResizeObserver(entries => {
      entries.forEach(entry => {
        if (entry.target === target) {
          updateDimensions();
        }
      });
    });

    // Children might resize without re-rendering the tooltip.
    // Observe that and trigger re-positioning.
    // Unlike useLayoutEffect, the ResizeObserver does not prevent
    // rendering the old state.
    observer.observe(target);
    return () => observer.disconnect();
  }, [ref, deps]);

  return dimensions;
}
