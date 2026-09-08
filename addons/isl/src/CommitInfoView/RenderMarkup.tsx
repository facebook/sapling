/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {MouseEvent} from 'react';

import {useAtomValue} from 'jotai';
import MarkdownIt from 'markdown-it';
import {useEffect, useState} from 'react';
import {cached} from 'shared/LRU';
import clientToServerAPI from '../ClientToServerAPI';
import {codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {atomFamilyWeak, lazyAtom} from '../jotaiUtils';
import platform from '../platform';

import './RenderedMarkup.css';

/** Holds a relative image path until its contents are loaded from the server. */
const LOCAL_SRC_ATTR = 'data-local-src';
const IMAGE_ATTRS = new Set(['src', 'alt', 'width', 'height']);
const IMAGE_TAG = /^<img\b([^<>]*?)\s*\/?>$/i;
const IMAGE_ATTR = /([a-z-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/gi;
const REMOTE_IMAGE_SOURCE = /^(https?:\/\/|data:image\/)/i;

function isLocalImageSource(src: string): boolean {
  return src !== '' && !src.startsWith('/') && !/^[a-z][a-z0-9+.-]*:/i.test(src);
}

const markdown = new MarkdownIt({html: true, linkify: true});

/**
 * Rebuild a raw `<img>` tag from its image attributes alone, or return null for any other HTML.
 * Everything that is not an image renders as text, so a `<Component>` name in a description
 * stays visible.
 */
function renderImageTag(html: string): string | null {
  const match = IMAGE_TAG.exec(html);
  if (match == null) {
    return null;
  }
  const attrs = new Map<string, string>();
  for (const [, name, double, single, bare] of match[1].matchAll(IMAGE_ATTR)) {
    const key = name.toLowerCase();
    if (IMAGE_ATTRS.has(key)) {
      attrs.set(key, markdown.utils.unescapeAll(double ?? single ?? bare ?? ''));
    }
  }
  const src = attrs.get('src');
  if (src == null) {
    return null;
  }
  if (isLocalImageSource(src)) {
    attrs.delete('src');
    attrs.set(LOCAL_SRC_ATTR, src);
  } else if (!REMOTE_IMAGE_SOURCE.test(src)) {
    return null;
  }
  const rendered = [...attrs]
    .map(([key, value]) => ` ${key}="${markdown.utils.escapeHtml(value)}"`)
    .join('');
  return `<img${rendered}>`;
}

markdown.renderer.rules.html_inline = (tokens, idx) =>
  renderImageTag(tokens[idx].content) ?? markdown.utils.escapeHtml(tokens[idx].content);
markdown.renderer.rules.html_block = (tokens, idx) => {
  const content = tokens[idx].content.trim();
  return `<p>${renderImageTag(content) ?? markdown.utils.escapeHtml(content)}</p>\n`;
};

const renderImage = markdown.renderer.rules.image;
markdown.renderer.rules.image = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const src = token.attrGet('src');
  if (src != null && isLocalImageSource(src)) {
    token.attrs = token.attrs?.filter(([name]) => name !== 'src') ?? null;
    token.attrSet(LOCAL_SRC_ATTR, src);
  }
  return renderImage?.(tokens, idx, options, env, self) ?? self.renderToken(tokens, idx, options);
};

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

const localImageRequests = new Map<string, Promise<string>>();
const loadedLocalImages = new Map<string, string>();

/** Load a relative image path as a `data:` URL, since the webview cannot read the disk itself. */
function loadLocalImage(src: string): Promise<string> {
  let image = localImageRequests.get(src);
  if (image == null) {
    requestId += 1;
    const id = requestId;
    clientToServerAPI.postMessage({type: 'fetchLocalImage', src, id});
    image = clientToServerAPI
      .nextMessageMatching('fetchedLocalImage', message => message.id === id)
      .then(({result}) => {
        if (result.value == null) {
          throw result.error ?? new Error(`Could not load ${src}`);
        }
        loadedLocalImages.set(src, result.value);
        return result.value;
      });
    // Let a later render retry, such as after the file is created.
    image.catch(() => localImageRequests.delete(src));
    localImageRequests.set(src, image);
  }
  return image;
}

const LOCAL_SRC_PATTERN = new RegExp(`${LOCAL_SRC_ATTR}="([^"]*)"`, 'g');
const ESCAPED_CHARACTERS: Record<string, string> = {
  '&amp;': '&',
  '&lt;': '<',
  '&gt;': '>',
  '&quot;': '"',
};

function unescapeAttribute(value: string): string {
  return value.replace(/&(amp|lt|gt|quot);/g, entity => ESCAPED_CHARACTERS[entity]);
}

/**
 * Swap each loaded relative image into the HTML as its `data:` URL. Keeping the image in the HTML
 * string, rather than setting it on the DOM, means a re-render that rewrites the markup keeps it.
 */
function withLoadedImages(html: string): {html: string; pending: Array<string>} {
  const pending: Array<string> = [];
  const replaced = html.replace(LOCAL_SRC_PATTERN, (attr, escapedSrc: string) => {
    const src = unescapeAttribute(escapedSrc);
    const dataUrl = loadedLocalImages.get(src);
    if (dataUrl == null) {
      pending.push(src);
      return attr;
    }
    return `src="${dataUrl}"`;
  });
  return {html: replaced, pending};
}

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
  const [, setLoadedCount] = useState(0);
  const {html, pending} = withLoadedImages(renderedHtml ?? '');
  const pendingKey = pending.join('\n');

  useEffect(() => {
    let mounted = true;
    for (const src of pendingKey === '' ? [] : pendingKey.split('\n')) {
      loadLocalImage(src).then(
        () => mounted && setLoadedCount(count => count + 1),
        () => undefined,
      );
    }
    return () => {
      mounted = false;
    };
  }, [pendingKey]);

  // The HTML comes either from the trusted server or from markdown-it, which passes through no raw
  // HTML but rebuilt `<img>` tags, so it is injected without further sanitizing.
  return html !== '' ? (
    <div
      className="rendered-markup"
      onClick={openLinksExternally}
      dangerouslySetInnerHTML={{__html: html}}
    />
  ) : (
    <div>{children}</div>
  );
}
