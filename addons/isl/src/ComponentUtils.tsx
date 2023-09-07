/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Icon} from 'shared/Icon';

import './ComponentUtils.css';

export function LargeSpinner() {
  return (
    <div data-testid="loading-spinner">
      <Icon icon="loading" size="L" />
    </div>
  );
}

export function Center(props: ContainerProps) {
  const className = addClassName('center-container', props);
  return (
    <div {...props} className={className}>
      {props.children}
    </div>
  );
}

export function FlexRow(props: FlexProps) {
  return FlexBox({...props, className: addClassName('flex-row', props)});
}

/** Flexbox container with horizontal children. */
export function Row(props: FlexProps) {
  return FlexBox({...props, direction: 'row'});
}

/** Flexbox container with vertical children. */
export function Column(props: FlexProps) {
  return FlexBox({...props, direction: 'column'});
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
  return <div className={'spacer'} />;
}

type ContainerProps = React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>;

type FlexProps = ContainerProps & {
  direction?: 'row' | 'column';
};

/** See `<Row>` and `<Column>`. */
function FlexBox(props: FlexProps) {
  const direction = props.direction ?? 'row';
  const style: React.CSSProperties = {
    display: 'flex',
    flexDirection: direction,
    flexWrap: 'nowrap',
  };
  const mergedProps = {...props, style: {...style, ...props.style}};
  delete mergedProps.children;
  delete mergedProps.direction;
  return <div {...mergedProps}>{props.children}</div>;
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

  // The outter <div> seems to avoid issues where
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

function addClassName(name: string, props: FlexProps) {
  return props.className == null ? name : `${props.className} ${name}`;
}
