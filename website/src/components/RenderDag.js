/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React from 'react';

// Matches bitflags! { LinkLine } in Rust.
const LinkLine = {
  HORIZ_PARENT: 0b0_0000_0000_0001,
  HORIZ_ANCESTOR: 0b0_0000_0000_0010,
  VERT_PARENT: 0b0_0000_0000_0100,
  VERT_ANCESTOR: 0b0_0000_0000_1000,
  LEFT_FORK_PARENT: 0b0_0000_0001_0000,
  LEFT_FORK_ANCESTOR: 0b0_0000_0010_0000,
  RIGHT_FORK_PARENT: 0b0_0000_0100_0000,
  RIGHT_FORK_ANCESTOR: 0b0_0000_1000_0000,
  LEFT_MERGE_PARENT: 0b0_0001_0000_0000,
  LEFT_MERGE_ANCESTOR: 0b0_0010_0000_0000,
  RIGHT_MERGE_PARENT: 0b0_0100_0000_0000,
  RIGHT_MERGE_ANCESTOR: 0b0_1000_0000_0000,
  CHILD: 0b1_0000_0000_0000,
};

function empty() {
  return <div />;
}

// Render dag into svg.
export default function RenderDag({
  dag,
  subset,
  style,
  circleRadius = 16,
  padLineHeight = 4,
  linkLineHeight = 10,
  columnWidth = 14,
  padding = 4,
  bypassSize = 4,
  dashArray = "4,2",
  rotate = true,
  drawExtra,
}) {
  // Output
  const svgCircles = [];
  const svgPaths = [];
  const svgTexts = [];
  const circles = new Map();

  // Rotate helpers, (x, y) => (-y, -x)
  const xys = (x, y) => rotate ? `${-y} ${-x}` : `${x} ${y}`;
  const xyt = (x, y) => rotate ? [-y, -x] : [x, y];

  // Aliases
  const r = circleRadius;
  const dx = columnWidth;

  // State
  let dy = circleRadius;
  let maxY = 0;
  let maxX = 0;
  let minY = 0;
  let minX = 0;
  let x = padding;
  let y = padding;

  function updateViewbox(x, y) {
    if (x > maxX) {
      maxX = x + padding;
    }
    if (x < minX) {
      minX = x - padding;
    }
    if (y > maxY) {
      maxY = y + padding;
    }
    if (y < minY) {
      minY = y + padding;
    }
  }

  function stepX() {
    x += dx * 2;
    updateViewbox(x, y);
  }

  function stepY() {
    updateViewbox(x, y);
    y += dy * 2;
  }

  // drawLine in a "cell". dx and dy range from -1 to 1. (0, 0) is the center.
  function drawLine(dx1, dy1, dx2, dy2, dashed) {
    const [x1, y1] = xyt(x + (dx1 + 1) * dx, y + (dy1 + 1) * dy);
    const [x2, y2] = xyt(x + (dx2 + 1) * dx, y + (dy2 + 1) * dy);
    const dash = dashed ? dashArray : null;
    const key = `${x}.${y}.${dx1}.${dy1}.${dx2}.${dy2}`;
    let d = '';
    if (y1 === y2 || x1 === x2) {
      // Straight line.
      d = `M ${x1} ${y1} L ${x2} ${y2}`;
    } else {
      // Curved line (towards center).
      const [qx, qy] = xyt(x + dx, y + dy);
      d = `M ${x1} ${y1} Q ${qx} ${qy}, ${x2} ${y2}`;
    }
    svgPaths.push(<path d={d} key={key} strokeDasharray={dash} />);
  }

  function drawVerticalBypassLine(dashed) {
    const dash = dashed ? dashArray : null;
    svgPaths.push(<path d={`M ${xys(x + dx, y)} l ${xys(0, dy - bypassSize)} q ${xys(bypassSize, bypassSize)}, ${xys(0, bypassSize * 2)} l ${xys(0, dy - bypassSize)}`} strokeDasharray={dash} key={`b${x}.${y}`} />);
  }

  function drawCircle(name) {
    const [cx, cy] = xyt(x + dx, y + dy);
    svgCircles.push(<circle cx={cx} cy={cy} r={r} key={name} />);
    svgTexts.push(<text x={cx} y={cy} textAnchor="middle" alignmentBaseline="middle" key={name}>{name}</text>);
    circles.set(name, { cx, cy, name });
  }

  function drawNodeOrPadLines(lines, nodeGlyph=null) {
    const needDashLine = lines.some((l) => l == "Ancestor");
    dy = nodeGlyph ? circleRadius : (needDashLine ? linkLineHeight : padLineHeight);
    x = padding;
    for (const line of lines) {
      switch (line) {
        case "Ancestor":
          drawLine(0, -1, 0, 1, true);
          break;
        case "Parent":
          drawLine(0, -1, 0, 1);
          break;
        case "Node":
          drawCircle(nodeGlyph);
          break;
        case "Blank":
          break;
      }
      stepX();
    }
    stepY();
  }

  function drawLinkLines(lines) {
    dy = linkLineHeight;
    x = padding;
    for (const line of lines) {
      const {bits} = line;
      function maybeDrawLine(parentBits, ancestorBits, drawFunc) {
        const present = (bits & (parentBits | ancestorBits)) !== 0;
        const dashed = (bits & ancestorBits) !== 0;
        if (present) {
          drawFunc(dashed);
        }
      }
      maybeDrawLine(LinkLine.HORIZ_PARENT, LinkLine.HORIZ_ANCESTOR, (dashed) => {
        drawLine(-1, 0, 1, 0, dashed);
      });
      maybeDrawLine(LinkLine.LEFT_MERGE_PARENT, LinkLine.LEFT_MERGE_ANCESTOR, (dashed) => {
        drawLine(-1, 0, 0, -1, dashed);
      });
      maybeDrawLine(LinkLine.RIGHT_MERGE_PARENT, LinkLine.RIGHT_MERGE_ANCESTOR, (dashed) => {
        drawLine(1, 0, 0, -1, dashed);
      });
      maybeDrawLine(LinkLine.LEFT_FORK_PARENT, LinkLine.LEFT_FORK_ANCESTOR, (dashed) => {
        drawLine(-1, 0, 0, 1, dashed);
      });
      maybeDrawLine(LinkLine.RIGHT_FORK_PARENT, LinkLine.RIGHT_FORK_ANCESTOR, (dashed) => {
        drawLine(1, 0, 0, 1, dashed);
      });
      maybeDrawLine(LinkLine.VERT_PARENT, LinkLine.VERT_ANCESTOR, (dashed) => {
        if (bits & (LinkLine.HORIZ_PARENT | LinkLine.HORIZ_ANCESTOR)) {
          drawVerticalBypassLine(dashed);
        } else {
          drawLine(0, -1, 0, 1, dashed);
        }
      });
      stepX();
    }
    stepY();
  }

  if (!dag) {
    return empty();
  }

  let rows = null;
  if (subset) {
    try {
      rows = dag.renderSubset(subset);
    } catch {
      // Invalid subset. For example, contains unknown nodes.
      rows = dag.render();
    }
  } else {
    rows = dag.render();
  }
  for (const row of rows) {
    // See GraphRow in dag/src/render/render.rs for definition of `row`.
    drawNodeOrPadLines(row.node_line, row.glyph);
    if (row.link_line) {
      drawLinkLines(row.link_line);
    }
    drawNodeOrPadLines(row.pad_lines);
  }

  let svgExtra = null;
  if (drawExtra) {
    svgExtra = drawExtra({circles, r, updateViewbox: (x, y) => {
      const [xt, yt] = xyt(x, y);
      updateViewbox(xt, yt);
    }});
  }

  function calcBounds() {
    const [x1, y1] = xyt(minX, minY);
    const [x2, y2] = xyt(maxX, maxY);
    const x = Math.min(x1, x2);
    const y = Math.min(y1, y2);
    const width = Math.abs(x2 - x1);
    const height = Math.abs(y2 - y1);
    return {
      height,
      viewBox: `${x} ${y} ${width} ${height}`,
      width,
    }
  }

  const {viewBox, height, width} = calcBounds();
  if (width === 0 || height === 0) {
    return empty();
  }

  const mergedStyle = {
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
    display: 'flex',
    ...style
  };

  return <div className="svgdag" style={mergedStyle}>
    <svg viewBox={viewBox} width={Math.abs(width)}>
      <g stroke="var(--ifm-color-primary-darkest)" fill="none" strokeWidth={2}>
        {svgPaths}
      </g>
      <g stroke="var(--ifm-color-primary-darkest)" fill="var(--ifm-color-primary)" strokeWidth={2}>
        {svgCircles}
      </g>
      <g stroke="none" fill="var(--ifm-color-content-inverse)">
        {svgTexts}
      </g>
      {svgExtra}
    </svg>
  </div>;
};

