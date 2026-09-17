/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommentLocation} from './commentLinkUtils';
import type {ID} from './github/types';

import {commentAnchorID, commentPermalink} from './commentLinkUtils';
import {notificationMessageAtom} from './jotai';
import {LinkIcon} from '@primer/octicons-react';
import {IconButton} from '@primer/react';
import {useSetAtom} from 'jotai';
import {useCallback, useEffect} from 'react';

type Props = {
  id: ID;
  location?: CommentLocation;
};

export default function CommentLink({id, location = 'timeline'}: Props): React.ReactElement {
  const setNotification = useSetAtom(notificationMessageAtom);
  const anchorID = commentAnchorID(id, location);

  useEffect(() => {
    const currentAnchor = decodeURIComponent(window.location.hash.slice(1));
    if (currentAnchor === anchorID) {
      requestAnimationFrame(() => {
        document.getElementById(anchorID)?.scrollIntoView({block: 'center'});
      });
    }
  }, [anchorID]);

  const copyLink = useCallback(async () => {
    const permalink = commentPermalink(id, location);
    window.history.replaceState(null, '', permalink);
    document.getElementById(anchorID)?.scrollIntoView({block: 'center'});
    try {
      await navigator.clipboard.writeText(permalink);
      setNotification({type: 'info', message: 'Comment link copied.'});
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setNotification({type: 'error', message: `Failed to copy comment link: ${message}`});
    }
  }, [anchorID, id, location, setNotification]);

  return (
    <IconButton
      aria-label="Copy link to comment"
      icon={LinkIcon}
      onClick={copyLink}
      variant="invisible"
      sx={{height: '28px', minWidth: '28px', padding: 0}}
    />
  );
}
