/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {InternalTypes} from '../InternalTypes';
import type {CommitMessageFieldsUtilsType, FieldsBeingEdited} from './types';

import {Internal} from '../Internal';

function emptyCommitMessageFields(): OSSCommitMessageFields {
  return {
    title: '',
    description: '',
  };
}

function noFieldsBeingEdited(): FieldsBeingEdited<OSSCommitMessageFields> {
  return {
    title: false,
    description: false,
  };
}

function allFieldsBeingEdited(): FieldsBeingEdited<OSSCommitMessageFields> {
  return {
    title: true,
    description: true,
  };
}

function findFieldsBeingEdited(
  a: OSSCommitMessageFields,
  b: OSSCommitMessageFields,
): FieldsBeingEdited<OSSCommitMessageFields> {
  return {
    title: a.title !== b.title,
    description: a.description !== b.description,
  };
}

function parseCommitMessageFields(title: string, description: string): OSSCommitMessageFields {
  return {
    title,
    description,
  };
}

function commitMessageFieldsToString(fields: OSSCommitMessageFields): string {
  return `${fields.title}\n${fields.description}`;
}

type OSSCommitMessageFields = {
  title: string;
  description: string;
};

export const OSSCommitMessageFieldsUtils: CommitMessageFieldsUtilsType<OSSCommitMessageFields> = {
  emptyCommitMessageFields,
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

/** Utilities to access, parse, and use CommitMessageFields. If internal, uses internal fields. */
/** type of parseable fields from commit messages. If internal, includes internal fields. */
export type CommitMessageFields = InternalTypes['InternalCommitMessageFields'] extends never
  ? OSSCommitMessageFields
  : InternalTypes['InternalCommitMessageFields'];

/**
 * Type representing fields parsed from a commit message.
 * Internally, this includes summary and test plan, etc.
 * Externally, this is just the description right now
 * TODO: Support defining this via a config so OSS users can get the fields they want in each repo.
 */
export const CommitMessageFieldUtils = (Internal.CommitMessageFieldUtils ??
  OSSCommitMessageFieldsUtils) as CommitMessageFieldsUtilsType<CommitMessageFields>;
