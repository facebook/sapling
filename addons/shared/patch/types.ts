/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export type Hunk = {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
  linedelimiters: string[];
};

export enum DiffType {
  Modified = 'Modified',
  Added = 'Added',
  Removed = 'Removed',
  Renamed = 'Renamed',
  Copied = 'Copied',
}

export type ParsedDiff = {
  type?: DiffType;
  oldFileName?: string;
  newFileName?: string;
  oldMode?: string;
  newMode?: string;
  hunks: Hunk[];
};
