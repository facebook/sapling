/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useEffect, useState} from 'react';
import {randomId} from 'shared/utils';
import clientToServerAPI from './ClientToServerAPI';

export type Heartbeat =
  | {
      type: 'waiting';
    }
  | {
      type: 'timeout';
    }
  | {
      type: 'success' | 'slow';
      /** Round trip time between sending the heartbeat and getting the response */
      rtt: number;
    };

export const DEFAULT_HEARTBEAT_TIMEOUT_MS = 1500;

/**
 * Ping the server
 */
export function useHeartbeat(timeoutMs = DEFAULT_HEARTBEAT_TIMEOUT_MS) {
  const [state, setState] = useState<Heartbeat>({type: 'waiting'});

  useEffect(() => {
    const id = randomId();
    const start = Date.now();
    clientToServerAPI.postMessage({type: 'heartbeat', id});
    clientToServerAPI
      .nextMessageMatching('heartbeat', message => message.id === id)
      .then(() => {
        setState(val => {
          if (val.type === 'waiting') {
            return {type: 'success', rtt: Date.now() - start};
          } else if (val.type === 'timeout') {
            return {type: 'slow', rtt: Date.now() - start};
          }
          return val;
        });
      });

    const timeout = setTimeout(() => {
      setTimeout(() => {
        setState(val => {
          if (val.type === 'waiting') {
            return {type: 'timeout'};
          }
          return val;
        });
      });
    }, timeoutMs);

    return () => clearTimeout(timeout);
  }, [setState, timeoutMs]);

  return state;
}
