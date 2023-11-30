/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import clientToServerAPI from '../ClientToServerAPI';
import {codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {selectorFamily, useRecoilValueLoadable} from 'recoil';

import './RenderedMarkup.css';

let requestId = 0;
const renderedMarkup = selectorFamily<string, string>({
  key: 'renderedMarkup',
  get:
    markup =>
    ({get}) => {
      const provider = get(codeReviewProvider);
      if (provider?.enableMessageSyncing !== true) {
        return markup;
      }
      requestId += 1;
      const id = requestId;
      clientToServerAPI.postMessage({type: 'renderMarkup', markup, id});
      return clientToServerAPI
        .nextMessageMatching('renderedMarkup', message => message.id === id)
        .then(message => message.html);
    },
});

export function RenderMarkup({children}: {children: string}) {
  const renderedHtml = useRecoilValueLoadable(renderedMarkup(children)).valueMaybe();
  // TODO: We could consider using DOM purify to sanitize this HTML,
  // though this html is coming directly from a trusted server.
  return renderedHtml != null ? (
    <div className="rendered-markup" dangerouslySetInnerHTML={{__html: renderedHtml}} />
  ) : (
    <div>{children}</div>
  );
}
