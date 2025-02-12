/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {nextTick} from 'shared/testUtils';
import clientToServerAPI from '../ClientToServerAPI';
import {resetTestMessages, simulateMessageFromServer} from '../testUtils';

describe('ClientToServer', () => {
  beforeEach(() => {
    resetTestMessages();
  });

  describe('nextMessageMatching', () => {
    it('resolves when it sees a matching message', async () => {
      let isResolved = false;
      const matchingPromise = clientToServerAPI.nextMessageMatching(
        'uploadFileResult',
        message => message.id === '1234',
      );

      matchingPromise.then(() => {
        isResolved = true;
      });

      simulateMessageFromServer({type: 'beganLoadingMoreCommits'}); // doesn't match type
      simulateMessageFromServer({type: 'uploadFileResult', result: {value: 'hi'}, id: '9999'}); // doesn't match predicate
      await nextTick();
      expect(isResolved).toEqual(false);

      simulateMessageFromServer({type: 'uploadFileResult', result: {value: 'hi'}, id: '1234'}); // matches
      expect(matchingPromise).resolves.toEqual({
        type: 'uploadFileResult',
        result: {value: 'hi'},
        id: '1234',
      });

      simulateMessageFromServer({type: 'uploadFileResult', result: {value: 'hi'}, id: '1234'}); // doesn't crash or anything if another message would match
    });
  });
});
