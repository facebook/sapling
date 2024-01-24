/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {T} from '../i18n';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useLayoutEffect, useState, useRef} from 'react';
import {Icon} from 'shared/Icon';

import './SeeMoreContainer.css';

const MAX_NON_EXPANDBLE_HEIGHT_PX = 275;

export function SeeMoreContainer({
  children,
  maxHeight,
  className,
}: {
  children: ReactNode;
  maxHeight?: number;
  className?: string;
}) {
  const ref = useRef<null | HTMLDivElement>(null);
  const [shouldShowExpandButton, setShouldShowExpandbutton] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);

  useLayoutEffect(() => {
    const element = ref.current;
    if (element != null && element.scrollHeight > (maxHeight ?? MAX_NON_EXPANDBLE_HEIGHT_PX)) {
      shouldShowExpandButton === false && setShouldShowExpandbutton(true);
    } else {
      shouldShowExpandButton === true && setShouldShowExpandbutton(false);
    }
    // Weird: we pass children to trick it to rerun this effect when the selected commit changes
    // We could also do this by passing a new key to <SeeMoreContainer> in the caller
  }, [ref, shouldShowExpandButton, children, maxHeight]);

  return (
    <div className={'see-more-container ' + (className ?? '')}>
      <div
        className={`see-more-container-${isExpanded ? 'expanded' : 'collapsed'}`}
        ref={ref}
        style={{maxHeight: isExpanded ? undefined : maxHeight}}>
        {children}
      </div>
      {shouldShowExpandButton ? (
        <div className={`see-${isExpanded ? 'less' : 'more'}-button`}>
          <VSCodeButton appearance="icon" onClick={() => setIsExpanded(val => !val)}>
            <Icon icon={isExpanded ? 'fold-up' : 'fold-down'} slot="start" />
            {isExpanded ? <T>See less</T> : <T>See more</T>}
          </VSCodeButton>
        </div>
      ) : null}
    </div>
  );
}
