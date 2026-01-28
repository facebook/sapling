/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {useCallback, useEffect, useRef, useState} from 'react';
import {notEmpty} from 'shared/utils';
import {spacing} from '../../components/theme/tokens.stylex';

import './ComponentUtils.css';

const styles = stylex.create({
  center: {
    display: 'flex',
    width: '100%',
    height: '100%',
    alignItems: 'center',
    justifyContent: 'center',
  },
  flex: {
    display: 'flex',
    alignItems: 'center',
    gap: spacing.pad,
  },
  spacer: {
    flexGrow: 1,
  },
  alignStart: {
    alignItems: 'flex-start',
  },
});

export type ReactProps<T extends HTMLElement> = React.DetailedHTMLProps<React.HTMLAttributes<T>, T>;

export function LargeSpinner() {
  return (
    <div data-testid="loading-spinner">
      <Icon icon="loading" size="L" />
    </div>
  );
}

export function Center(props: ContainerProps) {
  const {className, xstyle, ...rest} = props;
  return (
    <div
      {...stylexPropsWithClassName([styles.center, xstyle].filter(notEmpty), className)}
      {...rest}
    />
  );
}

/** Flexbox container with horizontal children. */
export function Row(props: ContainerProps) {
  return FlexBox(props, 'row');
}

/** Flexbox container with vertical children. */
export function Column(props: ContainerProps) {
  return FlexBox(props, 'column');
}

/** Container that scrolls horizontally. */
export function ScrollX(props: ScrollProps) {
  return Scroll({...props, direction: 'x'});
}

/** Container that scrolls vertically. */
export function ScrollY(props: ScrollProps) {
  return Scroll({...props, direction: 'y'});
}

/** Visually empty flex item with `flex-grow: 1` to insert as much space as possible between siblings. */
export function FlexSpacer() {
  return <div {...stylex.props(styles.spacer)} />;
}

type ContainerProps = ReactProps<HTMLDivElement> & {xstyle?: stylex.StyleXStyles} & {
  /** If true, use alignItems: flex-start instead of centering */
  alignStart?: boolean;
};

/** See `<Row>` and `<Column>`. */
function FlexBox(props: ContainerProps, flexDirection: 'row' | 'column') {
  const {className, style, alignStart, xstyle, ...rest} = props;
  return (
    <div
      {...stylexPropsWithClassName(
        [styles.flex, alignStart && styles.alignStart, xstyle].filter(notEmpty),
        className,
      )}
      {...rest}
      style={{flexDirection, ...style}}
    />
  );
}

type ScrollProps = ContainerProps & {
  /** Scroll direction. */
  direction?: 'x' | 'y';
  /** maxHeight or maxWidth depending on scroll direction. */
  maxSize?: string | number;
  /** height or width depending on scroll direction. */
  size?: string | number;
  /** Whether to hide the scroll bar. */
  hideBar?: boolean;
  /** On-scroll event handler. */
  onScroll?: React.UIEventHandler;
};

/** See <ScrollX> and <ScrollY> */
function Scroll(props: ScrollProps) {
  let className = props.className ?? '';
  const direction = props.direction ?? 'x';
  const hideBar = props.hideBar ?? false;
  const style: React.CSSProperties = {};
  if (direction === 'x') {
    style.overflowX = 'auto';
    style.maxWidth = props.maxSize ?? '100%';
    if (props.size != null) {
      style.width = props.size;
    }
  } else {
    style.overflowY = 'auto';
    style.maxHeight = props.maxSize ?? '100%';
    if (props.size != null) {
      style.height = props.size;
    }
  }
  if (hideBar) {
    style.scrollbarWidth = 'none';
    className += ' hide-scrollbar';
  }

  const mergedProps = {...props, className, style: {...style, ...props.style}};
  delete mergedProps.children;
  delete mergedProps.maxSize;
  delete mergedProps.hideBar;
  delete mergedProps.direction;

  // The outer <div> seems to avoid issues where
  // the other direction of scrollbar gets used.
  // See https://pxl.cl/3bvWh for the difference.
  // I don't fully understand how this works exactly.
  // See also https://stackoverflow.com/a/6433475.
  return (
    <div style={{overflow: 'visible'}}>
      <div {...mergedProps}>{props.children}</div>
    </div>
  );
}

/**
 * Like stylex.props(), but also adds in extra classNames.
 * Useful since `{...stylex.props()}` sets className,
 * and either overwrites or is overwritten by other `className="..."` props.
 */
export function stylexPropsWithClassName(
  style: stylex.StyleXStyles,
  ...names: Array<string | undefined>
) {
  const {className, ...rest} = stylex.props(style);
  return {...rest, className: className + ' ' + names.filter(notEmpty).join(' ')};
}

/**
 * Hook to manage scroll fade indicators on a scrollable element.
 * Returns a ref to attach to the scrollable element and data attributes
 * to control the fade visibility.
 *
 * The fades show when there's content above/below the visible area.
 *
 * Usage:
 * - Attach `scrollRef` to the element that scrolls
 * - Spread `scrollProps` on the element with the CSS fade pseudo-elements
 *   (can be the same element or a parent container)
 */
export function useScrollFade<T extends HTMLElement = HTMLDivElement>(): {
  scrollRef: React.RefObject<T | null>;
  canScrollUp: boolean;
  canScrollDown: boolean;
  scrollProps: {
    onScroll: React.UIEventHandler<T>;
    'data-can-scroll-up': string;
    'data-can-scroll-down': string;
  };
} {
  const scrollRef = useRef<T>(null);
  const [canScrollUp, setCanScrollUp] = useState(false);
  const [canScrollDown, setCanScrollDown] = useState(false);

  const updateScrollState = useCallback(() => {
    const el = scrollRef.current;
    if (!el) {
      return;
    }

    const threshold = 5; // Small threshold to avoid floating point issues
    const scrollTop = el.scrollTop;
    const scrollHeight = el.scrollHeight;
    const clientHeight = el.clientHeight;

    setCanScrollUp(scrollTop > threshold);
    setCanScrollDown(scrollTop + clientHeight < scrollHeight - threshold);
  }, []);

  // Handle scroll events
  const handleScroll = useCallback(() => {
    updateScrollState();
  }, [updateScrollState]);

  // Initial check and resize observer
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) {
      return;
    }

    // Initial state
    updateScrollState();

    // Watch for resize changes
    const resizeObserver = new ResizeObserver(() => {
      updateScrollState();
    });
    resizeObserver.observe(el);

    // Watch for content changes via mutation observer
    const mutationObserver = new MutationObserver(() => {
      updateScrollState();
    });
    mutationObserver.observe(el, {childList: true, subtree: true});

    return () => {
      resizeObserver.disconnect();
      mutationObserver.disconnect();
    };
  }, [updateScrollState]);

  return {
    scrollRef,
    canScrollUp,
    canScrollDown,
    scrollProps: {
      onScroll: handleScroll,
      'data-can-scroll-up': String(canScrollUp),
      'data-can-scroll-down': String(canScrollDown),
    },
  };
}

/**
 * Wrapper component that adds scroll fade indicators to its children.
 * The children should be a scrollable element.
 */
export function ScrollFadeContainer({
  children,
  className,
  fadeHeight = 12,
  ...props
}: {
  children: React.ReactNode;
  className?: string;
  fadeHeight?: number;
} & React.HTMLAttributes<HTMLDivElement>) {
  const {scrollRef, scrollProps} = useScrollFade<HTMLDivElement>();

  return (
    <div
      ref={scrollRef}
      className={`scroll-fade-container ${className ?? ''}`}
      style={{'--scroll-fade-height': `${fadeHeight}px`} as React.CSSProperties}
      {...scrollProps}
      {...props}>
      {children}
    </div>
  );
}
