/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';

/* eslint no-bitwise: 0 */
/* Translated from fbcode/eden/scm/lib/renderdag/src/render.rs */

enum ColumnType {
  Empty = 0,
  Blocked = 1,
  Reserved = 2,
  Ancestor = 3,
  Parent = 4,
}

type ColumnProps =
  | {
      type: ColumnType.Empty | ColumnType.Blocked;
      hash: undefined;
    }
  | {
      type: ColumnType.Reserved | ColumnType.Ancestor | ColumnType.Parent;
      hash: Hash;
    };

export class Column {
  constructor(private inner: ColumnProps = {type: ColumnType.Empty, hash: undefined}) {}

  static empty(): Column {
    return new Column();
  }

  get type(): ColumnType {
    return this.inner.type;
  }

  get hash(): undefined | Hash {
    return this.inner.hash;
  }

  matches(n: Hash): boolean {
    return this.hash === n;
  }

  isEmpty(): boolean {
    return this.type === ColumnType.Empty;
  }

  variant(): number {
    return this.type;
  }

  mergeColumn(other: Column): Column {
    return other.variant() > this.variant() ? other : this;
  }

  reset(): Column {
    return this.type === ColumnType.Blocked ? Column.empty() : this;
  }

  toNodeLine(): NodeLine {
    switch (this.type) {
      case ColumnType.Ancestor:
        return NodeLine.Ancestor;
      case ColumnType.Parent:
        return NodeLine.Parent;
      default:
        return NodeLine.Blank;
    }
  }

  toLinkLine(): LinkLine {
    switch (this.type) {
      case ColumnType.Ancestor:
        return LinkLine.from(LinkLine.VERT_ANCESTOR);
      case ColumnType.Parent:
        return LinkLine.from(LinkLine.VERT_PARENT);
      default:
        return LinkLine.empty();
    }
  }

  toPadLine(): PadLine {
    switch (this.type) {
      case ColumnType.Ancestor:
        return PadLine.Ancestor;
      case ColumnType.Parent:
        return PadLine.Parent;
      default:
        return PadLine.Blank;
    }
  }
}

class Columns {
  public inner: Array<Column>;

  constructor(columns?: Array<Column>) {
    this.inner = columns ?? [];
  }

  find(node: Hash): number | undefined {
    const index = this.inner.findIndex(column => column.matches(node));
    return index >= 0 ? index : undefined;
  }

  findEmpty(index?: number): number | undefined {
    if (index != null && this.inner.at(index)?.isEmpty()) {
      return index;
    }
    return this.firstEmpty();
  }

  firstEmpty(): number | undefined {
    const index = this.inner.findIndex(column => column.isEmpty());
    return index >= 0 ? index : undefined;
  }

  newEmpty(): number {
    const columns = this.inner;
    columns.push(Column.empty());
    return columns.length - 1;
  }

  reset(): void {
    let columns = this.inner;
    columns = columns.map(column => column.reset());
    while (columns.at(-1)?.isEmpty()) {
      columns.pop();
    }
    this.inner = columns;
  }

  merge(index: number, column: Column) {
    const columns = this.inner;
    columns[index] = columns[index].mergeColumn(column);
  }

  swap(index1: number, index2: number) {
    if (index1 !== index2) {
      const column1 = this.inner[index1];
      const column2 = this.inner[index2];
      this.inner[index1] = column2;
      this.inner[index2] = column1;
    }
  }
}

export enum AncestorType {
  Ancestor = 'Ancestor',
  Parent = 'Parent',
  Anonymous = 'Anonymous',
}

type AncestorProps =
  | {
      type: AncestorType.Ancestor | AncestorType.Parent;
      hash: Hash;
    }
  | {
      type: AncestorType.Anonymous;
      hash: undefined;
    };

export class Ancestor {
  constructor(private inner: AncestorProps = {type: AncestorType.Anonymous, hash: undefined}) {}

  toColumn(): Column {
    switch (this.inner.type) {
      case AncestorType.Ancestor:
        return new Column({type: ColumnType.Ancestor, hash: this.inner.hash});
      case AncestorType.Parent:
        return new Column({type: ColumnType.Parent, hash: this.inner.hash});
      case AncestorType.Anonymous:
        return new Column({type: ColumnType.Blocked, hash: undefined});
    }
  }

  id(): Hash | undefined {
    return this.inner.hash;
  }

  isDirect(): boolean {
    return this.inner.type !== AncestorType.Ancestor;
  }

  toLinkLine(direct: LinkLine, indirect: LinkLine): LinkLine {
    return this.isDirect() ? direct : indirect;
  }
}

type AncestorColumnBoundsProps = {
  target: number;
  minAncestor: number;
  minParent: number;
  maxParent: number;
  maxAncestor: number;
};

export class AncestorColumnBounds {
  constructor(private inner: AncestorColumnBoundsProps) {}

  static new(columns: Array<[number, Ancestor]>, target: number): AncestorColumnBounds | undefined {
    if (columns.length === 0) {
      return undefined;
    }
    const ancestorNumbers = [target, ...columns.map(([index]) => index)];
    const parentNumbers = [target, ...columns.filter(([, a]) => a.isDirect()).map(([i]) => i)];
    const minAncestor = Math.min(...ancestorNumbers);
    const maxAncestor = Math.max(...ancestorNumbers);
    const minParent = Math.min(...parentNumbers);
    const maxParent = Math.max(...parentNumbers);
    return new AncestorColumnBounds({
      target,
      minAncestor,
      minParent,
      maxParent,
      maxAncestor,
    });
  }

  get minAncestor(): number {
    return this.inner.minAncestor;
  }

  get minParent(): number {
    return this.inner.minParent;
  }

  get maxParent(): number {
    return this.inner.maxParent;
  }

  get maxAncestor(): number {
    return this.inner.maxAncestor;
  }

  get target(): number {
    return this.inner.target;
  }

  *range(): Iterable<number> {
    for (let i = this.minAncestor + 1; i < this.maxAncestor; ++i) {
      yield i;
    }
  }

  horizontalLine(index: number): LinkLine {
    if (index === this.target) {
      return LinkLine.empty();
    } else if (index > this.minParent && index < this.maxParent) {
      return LinkLine.from(LinkLine.HORIZ_PARENT);
    } else if (index > this.minAncestor && index < this.maxAncestor) {
      return LinkLine.from(LinkLine.HORIZ_ANCESTOR);
    } else {
      return LinkLine.empty();
    }
  }
}

export class LinkLine {
  constructor(public value = 0) {}

  /** This cell contains a horizontal line that connects to a parent. */
  static HORIZ_PARENT = 1 << 0;
  /** This cell contains a horizontal line that connects to an ancestor. */
  static HORIZ_ANCESTOR = 1 << 1;
  /** The descendent of this cell is connected to the parent. */
  static VERT_PARENT = 1 << 2;
  /** The descendent of this cell is connected to an ancestor. */
  static VERT_ANCESTOR = 1 << 3;
  /** The parent of this cell is linked in this link row and the child is to the left. */
  static LEFT_FORK_PARENT = 1 << 4;
  /** The ancestor of this cell is linked in this link row and the child is to the left. */
  static LEFT_FORK_ANCESTOR = 1 << 5;
  /** The parent of this cell is linked in this link row and the child is to the right. */
  static RIGHT_FORK_PARENT = 1 << 6;
  /** The ancestor of this cell is linked in this link row and the child is to the right. */
  static RIGHT_FORK_ANCESTOR = 1 << 7;
  /** The child of this cell is linked to parents on the left. */
  static LEFT_MERGE_PARENT = 1 << 8;
  /** The child of this cell is linked to ancestors on the left. */
  static LEFT_MERGE_ANCESTOR = 1 << 9;
  /** The child of this cell is linked to parents on the right. */
  static RIGHT_MERGE_PARENT = 1 << 10;
  /** The child of this cell is linked to ancestors on the right. */
  static RIGHT_MERGE_ANCESTOR = 1 << 11;
  /**
   * The target node of this link line is the child of this column.
   * This disambiguates between the node that is connected in this link line,
   * and other nodes that are also connected vertically.
   */
  static CHILD = 1 << 12;

  static HORIZONTAL = LinkLine.HORIZ_PARENT | LinkLine.HORIZ_ANCESTOR;
  static VERTICAL = LinkLine.VERT_PARENT | LinkLine.VERT_ANCESTOR;
  static LEFT_FORK = LinkLine.LEFT_FORK_PARENT | LinkLine.LEFT_FORK_ANCESTOR;
  static RIGHT_FORK = LinkLine.RIGHT_FORK_PARENT | LinkLine.RIGHT_FORK_ANCESTOR;
  static LEFT_MERGE = LinkLine.LEFT_MERGE_PARENT | LinkLine.LEFT_MERGE_ANCESTOR;
  static RIGHT_MERGE = LinkLine.RIGHT_MERGE_PARENT | LinkLine.RIGHT_MERGE_ANCESTOR;
  static ANY_MERGE = LinkLine.LEFT_MERGE | LinkLine.RIGHT_MERGE;
  static ANY_FORK = LinkLine.LEFT_FORK | LinkLine.RIGHT_FORK;
  static ANY_FORK_OR_MERGE = LinkLine.ANY_MERGE | LinkLine.ANY_FORK;

  static from(value: number): LinkLine {
    return new LinkLine(value);
  }

  static empty(): LinkLine {
    return new LinkLine(0);
  }

  valueOf(): number {
    return this.value;
  }

  contains(value: number): boolean {
    return (this.value & value) === value;
  }

  intersects(value: number): boolean {
    return (this.value & value) !== 0;
  }

  or(value: number): LinkLine {
    return LinkLine.from(this.value | value);
  }
}

export enum NodeLine {
  Blank,
  Ancestor,
  Parent,
  Node,
}

export enum PadLine {
  Blank,
  Ancestor,
  Parent,
}

/**
 * Output row for a "commit".
 *
 * Example line types:
 *
 * ```plain
 *   o      F                     // node line
 *   ├─┬─╮  very long message 1   // link line
 *   │ │ │  very long message 2   // term line
 *   │ │ ~  very long message 3   // term line
 *   │ │                          // pad line
 *   │ │    very long message 4   // pad line
 * ```
 */
export type GraphRow = {
  hash: Hash;
  merge: boolean;
  /**  The node columns for this row */
  nodeLine: Array<NodeLine>;
  /** The link columns for this row, if a link row is necessary */
  linkLine?: Array<LinkLine>;
  /**
   * The location of any terminators, if necessary.  Other columns should be
   * filled with pad lines.
   */
  termLine?: Array<boolean>;
  /** Pad columns */
  padLines: Array<PadLine>;
};

export class Renderer {
  private columns: Columns = new Columns();

  /**
   * Reserve a column for the given hash.
   * This is usually used to indent draft commits by reserving
   * columns for public commits.
   */
  reserve(hash: Hash) {
    if (this.columns.find(hash) == null) {
      const index = this.columns.firstEmpty();
      const column = new Column({type: ColumnType.Reserved, hash});
      if (index == null) {
        this.columns.inner.push(column);
      } else {
        this.columns.inner[index] = column;
      }
    }
  }

  /**
   * Render the next row.
   * Main logic of the renderer.
   */
  nextRow(hash: Hash, parents: Array<Ancestor>): GraphRow {
    // Find a column for this node.
    const column: number =
      this.columns.find(hash) ?? this.columns.firstEmpty() ?? this.columns.newEmpty();
    this.columns.inner[column] = Column.empty();

    // This row is for a merge if there are multiple parents.
    const merge = parents.length > 1;

    // Build the initial node line.
    const nodeLine: NodeLine[] = this.columns.inner.map(c => c.toNodeLine());
    nodeLine[column] = NodeLine.Node;

    // Build the initial link line.
    const linkLine: LinkLine[] = this.columns.inner.map(c => c.toLinkLine());
    let needLinkLine = false;

    // Build the initial term line.
    const termLine: boolean[] = this.columns.inner.map(_c => false);
    let needTermLine = false;

    // Build the initial pad line.
    const padLines: PadLine[] = this.columns.inner.map(c => c.toPadLine());

    // Assign each parent to a column.
    const parentColumns = new Map<number, Ancestor>();
    for (const p of parents) {
      // Check if the parent already has a column.
      const parentId = p.id();
      if (parentId != null) {
        const index = this.columns.find(parentId);
        if (index != null) {
          this.columns.merge(index, p.toColumn());
          parentColumns.set(index, p);
          continue;
        }
      }

      // Assign the parent to an empty column, preferring the column
      // the current node is going in, to maintain linearity.
      const index = this.columns.findEmpty(column);
      if (index != null) {
        this.columns.merge(index, p.toColumn());
        parentColumns.set(index, p);
        continue;
      }

      // There are no empty columns left. Make a new column.
      parentColumns.set(this.columns.inner.length, p);
      nodeLine.push(NodeLine.Blank);
      padLines.push(PadLine.Blank);
      linkLine.push(LinkLine.empty());
      termLine.push(false);
      this.columns.inner.push(p.toColumn());
    }

    // Mark parent columns with anonymous parents as terminating.
    for (const [i, p] of parentColumns.entries()) {
      if (p.id() == null) {
        termLine[i] = true;
        needTermLine = true;
      }
    }

    // Check if we can move the parent to the current column.
    if (parents.length === 1) {
      const [[parentColumn, parentAncestor]] = parentColumns.entries();
      if (parentColumn != null && parentColumn > column) {
        // This node has a single parent which was already
        // assigned to a column to the right of this one.
        // Move the parent to this column.
        this.columns.swap(column, parentColumn);
        parentColumns.delete(parentColumn);
        parentColumns.set(column, parentAncestor);
        // Generate a line from this column to the old
        // parent column.   We need to continue with the style
        // that was being used for the parent column.
        const wasDirect = linkLine.at(parentColumn)?.contains(LinkLine.VERT_PARENT);
        linkLine[column] = linkLine[column].or(
          wasDirect ? LinkLine.RIGHT_FORK_PARENT : LinkLine.RIGHT_FORK_ANCESTOR,
        );
        for (let i = column + 1; i < parentColumn; ++i) {
          linkLine[i] = linkLine[i].or(wasDirect ? LinkLine.HORIZ_PARENT : LinkLine.HORIZ_ANCESTOR);
        }
        linkLine[parentColumn] = LinkLine.from(
          wasDirect ? LinkLine.LEFT_MERGE_PARENT : LinkLine.LEFT_MERGE_ANCESTOR,
        );
        needLinkLine = true;
        // The pad line for the old parent column is now blank.
        padLines[parentColumn] = PadLine.Blank;
      }
    }

    // Connect the node column to all the parent columns.
    const bounds = AncestorColumnBounds.new([...parentColumns.entries()], column);
    if (bounds != null) {
      // If the parents extend beyond the columns adjacent to the node, draw a horizontal
      // ancestor line between the two outermost ancestors.
      for (const i of bounds.range()) {
        linkLine[i] = linkLine[i].or(bounds.horizontalLine(i).value);
        needLinkLine = true;
      }
      // If there is a parent or ancestor to the right of the node
      // column, the node merges from the right.
      if (bounds.maxParent > column) {
        linkLine[column] = linkLine[column].or(LinkLine.RIGHT_MERGE_PARENT);
        needLinkLine = true;
      } else if (bounds.maxAncestor > column) {
        linkLine[column] = linkLine[column].or(LinkLine.RIGHT_MERGE_ANCESTOR);
        needLinkLine = true;
      }
      // If there is a parent or ancestor to the left of the node column, the node merges from the left.
      if (bounds.minParent < column) {
        linkLine[column] = linkLine[column].or(LinkLine.LEFT_MERGE_PARENT);
        needLinkLine = true;
      } else if (bounds.minAncestor < column) {
        linkLine[column] = linkLine[column].or(LinkLine.LEFT_MERGE_ANCESTOR);
        needLinkLine = true;
      }
      // Each parent or ancestor forks towards the node column.
      for (const [i, p] of parentColumns.entries()) {
        padLines[i] = this.columns.inner[i].toPadLine();
        let orValue = 0;
        if (i < column) {
          orValue = p.toLinkLine(
            LinkLine.from(LinkLine.RIGHT_FORK_PARENT),
            LinkLine.from(LinkLine.RIGHT_FORK_ANCESTOR),
          ).value;
        } else if (i === column) {
          orValue =
            LinkLine.CHILD |
            p.toLinkLine(LinkLine.from(LinkLine.VERT_PARENT), LinkLine.from(LinkLine.VERT_ANCESTOR))
              .value;
        } else {
          orValue = p.toLinkLine(
            LinkLine.from(LinkLine.LEFT_FORK_PARENT),
            LinkLine.from(LinkLine.LEFT_FORK_ANCESTOR),
          ).value;
        }
        linkLine[i] = linkLine[i].or(orValue);
      }
    }

    // Now that we have assigned all the columns, reset their state.
    this.columns.reset();

    // Filter out the link line or term line if they are not needed.
    const optionalLinkLine = needLinkLine ? linkLine : undefined;
    const optionalTermLine = needTermLine ? termLine : undefined;

    return {
      hash,
      merge,
      nodeLine,
      linkLine: optionalLinkLine,
      termLine: optionalTermLine,
      padLines,
    };
  }
}
