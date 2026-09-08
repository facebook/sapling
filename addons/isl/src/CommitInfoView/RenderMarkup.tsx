/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useAtomValue} from 'jotai';
import MarkdownIt from 'markdown-it';
import type {MouseEvent} from 'react';
import {cached} from 'shared/LRU';
import clientToServerAPI from '../ClientToServerAPI';
import {codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import platform from '../platform';

import './RenderedMarkup.css';

// Raw HTML stays off so a `<Component>` name in a description shows as text rather than being
// dropped as an unknown tag.
const markdown = new MarkdownIt({html: false, linkify: true});

const renderedMarkup = atomFamilyWeak((markup: string) => {
  // This is an atom to trigger re-render when the server returns.
  return lazyAtom(get => {
    const provider = get(codeReviewProvider);
    if (provider?.enableMessageSyncing === true) {
      return renderMarkupToHTML(markup);
    }
    return markdown.render(markup);
  }, null);
});

let requestId = 0;

const renderMarkupToHTML = cached((markup: string): Promise<string> | string => {
  requestId += 1;
  const id = requestId;
  clientToServerAPI.postMessage({type: 'renderMarkup', markup, id});
  return new Promise(resolve => {
    clientToServerAPI
      .nextMessageMatching('renderedMarkup', message => message.id === id)
      .then(message => resolve(message.html));
  });
});

function openLinksExternally(event: MouseEvent<HTMLDivElement>) {
  const anchor = (event.target as Element).closest('a[href]');
  if (anchor instanceof HTMLAnchorElement) {
    // Keep the click from reaching the field's click-to-edit handler.
    event.preventDefault();
    event.stopPropagation();
    platform.openExternalLink(anchor.href);
  }
}

export function RenderMarkup({children}: {children: string}) {
  const renderedHtml = useAtomValue(renderedMarkup(children));
  // The HTML comes either from the trusted server or from markdown-it with raw HTML disabled,
  // so it is injected without further sanitizing.
  return renderedHtml != null && renderedHtml !== '' ? (
    <div
      className="rendered-markup"
      onClick={openLinksExternally}
      dangerouslySetInnerHTML={{__html: renderedHtml}}
    />
  ) : (
    <div>{children}</div>
  );
}
