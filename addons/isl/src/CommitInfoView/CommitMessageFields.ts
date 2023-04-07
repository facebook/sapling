/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CommitMessageFields,
  CommitMessageFieldsUtilsType,
  FieldConfig,
  FieldsBeingEdited,
} from './types';

import {Internal} from '../Internal';

export function emptyCommitMessageFields(schema: Array<FieldConfig>): CommitMessageFields {
  return Object.fromEntries(schema.map(config => [config.key, config.type === 'field' ? [] : '']));
}

function noFieldsBeingEdited(): FieldsBeingEdited {
  return {
    title: false,
    description: false,
  };
}

function allFieldsBeingEdited(): FieldsBeingEdited {
  return {
    title: true,
    description: true,
  };
}

function findFieldsBeingEdited(a: CommitMessageFields, b: CommitMessageFields): FieldsBeingEdited {
  return {
    title: a.title !== b.title,
    description: a.description !== b.description,
  };
}

function parseCommitMessageFields(title: string, description: string): CommitMessageFields {
  return {
    title,
    description,
  };
}

function commitMessageFieldsToString(fields: CommitMessageFields): string {
  return `${fields.title}\n${fields.description}`;
}

export const OSSCommitMessageFieldsUtils: CommitMessageFieldsUtilsType = {
  parseCommitMessageFields,
  commitMessageFieldsToString,

  configuredFields: [
    {key: 'title', name: 'Title', type: 'title', icon: 'milestone'},
    {key: 'description', name: 'Description', type: 'textarea', icon: 'note'},
  ],

  allFieldsBeingEdited,
  noFieldsBeingEdited,
  findFieldsBeingEdited,
};

/**
 * Type representing fields parsed from a commit message.
 * Internally, this includes summary and test plan, etc.
 * Externally, this is just the description right now
 */
export const CommitMessageFieldUtils: CommitMessageFieldsUtilsType =
  Internal.CommitMessageFieldUtils ?? OSSCommitMessageFieldsUtils;
