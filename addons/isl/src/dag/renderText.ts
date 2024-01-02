/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {Ancestor} from './render';

import {PadLine, LinkLine, NodeLine, Renderer} from './render';

/* eslint no-bitwise: 0 */
/* Translated from fbcode/eden/scm/lib/renderdag/src/box_drawing.rs */

const GLYPHYS = {
  SPACE: '  ',
  HORIZONTAL: '──',
  PARENT: '│ ',
  ANCESTOR: '╷ ',
  MERGE_LEFT: '╯ ',
  MERGE_RIGHT: '╰─',
  MERGE_BOTH: '┴─',
  FORK_LEFT: '╮ ',
  FORK_RIGHT: '╭─',
  FORK_BOTH: '┬─',
  JOIN_LEFT: '┤ ',
  JOIN_RIGHT: '├─',
  JOIN_BOTH: '┼─',
  TERMINATION: '~ ',
};

/** Render a graph to text (string) */
export class TextRenderer {
  private inner: Renderer;
  private extraPadLine: string | undefined = undefined;

  constructor() {
    this.inner = new Renderer();
  }

  reserve(hash: Hash) {
    this.inner.reserve(hash);
  }

  nextRow(hash: Hash, parents: Array<Ancestor>, message: string, glyph = 'o'): string {
    const line = this.inner.nextRow(hash, parents);
    const out: string[] = [];

    let needExtraPadLine = false;
    const messageLines = message.split('\n');
    const messageIter = messageLines.values();
    const pushWithMessageLine = (lineBuf: string[], msg?: string) => {
      const msgLine = msg ?? messageIter.next()?.value;
      if (msgLine != null) {
        lineBuf.push(' ');
        lineBuf.push(msgLine);
      }
      out.push(lineBuf.join('').trimEnd());
      out.push('\n');
    };

    // Render the previous extra pad line.
    if (this.extraPadLine != null) {
      out.push(this.extraPadLine);
      out.push('\n');
      this.extraPadLine = undefined;
    }

    // Render the node line.
    const outNodeLine: string[] = [];
    line.nodeLine.forEach(entry => {
      if (entry === NodeLine.Node) {
        outNodeLine.push(glyph);
        outNodeLine.push(' ');
      } else if (entry === NodeLine.Parent) {
        outNodeLine.push(GLYPHYS.PARENT);
      } else if (entry === NodeLine.Ancestor) {
        outNodeLine.push(GLYPHYS.ANCESTOR);
      } else if (entry === NodeLine.Blank) {
        outNodeLine.push(GLYPHYS.SPACE);
      }
    });
    pushWithMessageLine(outNodeLine);

    // Render the link line.
    const linkLine = line.linkLine;
    if (linkLine != null) {
      const outLinkLine = [];
      for (const cur of linkLine) {
        if (cur.intersects(LinkLine.HORIZONTAL)) {
          if (cur.intersects(LinkLine.CHILD)) {
            outLinkLine.push(GLYPHYS.JOIN_BOTH);
          } else if (cur.intersects(LinkLine.ANY_FORK) && cur.intersects(LinkLine.ANY_MERGE)) {
            outLinkLine.push(GLYPHYS.JOIN_BOTH);
          } else if (
            cur.intersects(LinkLine.ANY_FORK) &&
            cur.intersects(LinkLine.VERT_PARENT) &&
            !line.merge
          ) {
            outLinkLine.push(GLYPHYS.JOIN_BOTH);
          } else if (cur.intersects(LinkLine.ANY_FORK)) {
            outLinkLine.push(GLYPHYS.FORK_BOTH);
          } else if (cur.intersects(LinkLine.ANY_MERGE)) {
            outLinkLine.push(GLYPHYS.MERGE_BOTH);
          } else {
            outLinkLine.push(GLYPHYS.HORIZONTAL);
          }
        } else if (cur.intersects(LinkLine.VERT_PARENT) && !line.merge) {
          const left = cur.intersects(LinkLine.LEFT_MERGE | LinkLine.LEFT_FORK);
          const right = cur.intersects(LinkLine.RIGHT_MERGE | LinkLine.RIGHT_FORK);
          if (left && right) {
            outLinkLine.push(GLYPHYS.JOIN_BOTH);
          } else if (left) {
            outLinkLine.push(GLYPHYS.JOIN_LEFT);
          } else if (right) {
            outLinkLine.push(GLYPHYS.JOIN_RIGHT);
          } else {
            outLinkLine.push(GLYPHYS.PARENT);
          }
        } else if (
          cur.intersects(LinkLine.VERT_PARENT | LinkLine.VERT_ANCESTOR) &&
          !cur.intersects(LinkLine.LEFT_FORK | LinkLine.RIGHT_FORK)
        ) {
          const left = cur.intersects(LinkLine.LEFT_MERGE);
          const right = cur.intersects(LinkLine.RIGHT_MERGE);
          if (left && right) {
            outLinkLine.push(GLYPHYS.JOIN_BOTH);
          } else if (left) {
            outLinkLine.push(GLYPHYS.JOIN_LEFT);
          } else if (right) {
            outLinkLine.push(GLYPHYS.JOIN_RIGHT);
          } else if (cur.intersects(LinkLine.VERT_ANCESTOR)) {
            outLinkLine.push(GLYPHYS.ANCESTOR);
          } else {
            outLinkLine.push(GLYPHYS.PARENT);
          }
        } else if (
          cur.intersects(LinkLine.LEFT_FORK) &&
          cur.intersects(LinkLine.LEFT_MERGE | LinkLine.CHILD)
        ) {
          outLinkLine.push(GLYPHYS.JOIN_LEFT);
        } else if (
          cur.intersects(LinkLine.RIGHT_FORK) &&
          cur.intersects(LinkLine.RIGHT_MERGE | LinkLine.CHILD)
        ) {
          outLinkLine.push(GLYPHYS.JOIN_RIGHT);
        } else if (cur.intersects(LinkLine.LEFT_MERGE) && cur.intersects(LinkLine.RIGHT_MERGE)) {
          outLinkLine.push(GLYPHYS.MERGE_BOTH);
        } else if (cur.intersects(LinkLine.LEFT_FORK) && cur.intersects(LinkLine.RIGHT_FORK)) {
          outLinkLine.push(GLYPHYS.FORK_BOTH);
        } else if (cur.intersects(LinkLine.LEFT_FORK)) {
          outLinkLine.push(GLYPHYS.FORK_LEFT);
        } else if (cur.intersects(LinkLine.LEFT_MERGE)) {
          outLinkLine.push(GLYPHYS.MERGE_LEFT);
        } else if (cur.intersects(LinkLine.RIGHT_FORK)) {
          outLinkLine.push(GLYPHYS.FORK_RIGHT);
        } else if (cur.intersects(LinkLine.RIGHT_MERGE)) {
          outLinkLine.push(GLYPHYS.MERGE_RIGHT);
        } else {
          outLinkLine.push(GLYPHYS.SPACE);
        }
      }
      pushWithMessageLine(outLinkLine);
    }

    // Render the term lines.
    // For each column, if terminated, use "-" "~". Otherwise, use the pad line.
    const termLine = line.termLine;
    if (termLine != null) {
      const termStrs = [GLYPHYS.PARENT, GLYPHYS.TERMINATION];
      termStrs.forEach(termStr => {
        const termLineOut: string[] = [];
        termLine.forEach((term, i) => {
          if (term) {
            termLineOut.push(termStr);
          } else {
            termLineOut.push(toGlyph(line.padLines.at(i)));
          }
        });
        pushWithMessageLine(termLineOut);
      });
      needExtraPadLine = true;
    }

    // Render the pad lines for long messages.
    // basePadLine is the pad line columns, without text messages.
    const basePadLine: string[] = [];
    for (const entry of line.padLines) {
      basePadLine.push(toGlyph(entry));
    }

    for (const msg of messageIter) {
      const padLine: string[] = [...basePadLine];
      pushWithMessageLine(padLine, msg);
      needExtraPadLine = false;
    }

    if (needExtraPadLine) {
      this.extraPadLine = basePadLine.join('').trimEnd();
    }

    return out.join('');
  }
}

function toGlyph(pad?: PadLine): string {
  if (pad === PadLine.Parent) {
    return GLYPHYS.PARENT;
  } else if (pad === PadLine.Ancestor) {
    return GLYPHYS.ANCESTOR;
  } else {
    return GLYPHYS.SPACE;
  }
}
