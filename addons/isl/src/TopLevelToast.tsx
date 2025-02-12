/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useAtomValue} from 'jotai';
import {AnimatedReorderGroup} from './AnimatedReorderGroup';
import {hideToast, toastQueueAtom} from './toast';

import 'isl-components/Tooltip.css';
import './TopLevelToast.css';

export function TopLevelToast() {
  const toastQueue = useAtomValue(toastQueueAtom);

  const toastDivs = toastQueue.toArray().map(t => {
    const handleClick = () => hideToast([t.key]);
    return (
      <div className="toast tooltip" key={t.key} data-reorder-id={t.key} onClick={handleClick}>
        {t.message}
      </div>
    );
  });

  return <AnimatedReorderGroup className="toast-container">{toastDivs}</AnimatedReorderGroup>;
}
