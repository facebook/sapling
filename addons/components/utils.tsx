/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';

export type ReactProps<T extends HTMLElement> = React.DetailedHTMLProps<React.HTMLAttributes<T>, T>;

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
  return {...rest, className: className + ' ' + names.filter(name => name != null).join(' ')};
}

export function findParentWithClassName(
  start: HTMLElement,
  className: string,
): HTMLElement | undefined {
  let el = start as HTMLElement | null;
  while (el) {
    if (el.classList?.contains(className)) {
      return el;
    } else {
      el = el.parentElement;
    }
  }
  return undefined;
}
