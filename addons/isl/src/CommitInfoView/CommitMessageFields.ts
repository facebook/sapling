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

/**
 * Construct value representing all fields are false: {title: false, description: false, ...}
 */
export function noFieldsBeingEdited(schema: Array<FieldConfig>): FieldsBeingEdited {
  return Object.fromEntries(schema.map(config => [config.key, false]));
}

/**
 * Construct value representing all fields are being edited: {title: true, description: true, ...}
 */
export function allFieldsBeingEdited(schema: Array<FieldConfig>): FieldsBeingEdited {
  return Object.fromEntries(schema.map(config => [config.key, true]));
}

/**
 * Construct value representing which fields differ between two parsed messages, by comparing each field.
 * ```
 * findFieldsBeingEdited({title: 'hi', description: 'yo'}, {title: 'hey', description: 'yo'}) -> {title: true, description: false}
 * ```
 */
export function findFieldsBeingEdited(
  schema: Array<FieldConfig>,
  a: CommitMessageFields,
  b: CommitMessageFields,
): FieldsBeingEdited {
  return Object.fromEntries(
    schema.map(config => [
      config.key,
      config.type === 'field'
        ? !arraysEqual(a[config.key] as Array<string>, b[config.key] as Array<string>)
        : a[config.key] !== b[config.key],
    ]),
  );
}

function parseCommitMessageFields(title: string, description: string): CommitMessageFields {
  return {
    title,
    description,
  };
}

export function commitMessageFieldsToString(
  schema: Array<FieldConfig>,
  fields: CommitMessageFields,
): string {
  return schema
    .map(
      config =>
        // stringified messages of the form Key: value, except the title or generic description don't need a label
        (config.key === 'title' || config.key === 'description' ? '' : config.name + ': ') +
        (config.type === 'field'
          ? (fields[config.key] as Array<string>).join(', ')
          : fields[config.key]),
    )
    .join('\n\n');
}

export const OSSCommitMessageFieldsUtils: CommitMessageFieldsUtilsType = {
  parseCommitMessageFields,

  configuredFields: [
    {key: 'title', name: 'Title', type: 'title', icon: 'milestone'},
    {key: 'description', name: 'Description', type: 'textarea', icon: 'note'},
  ],
};

/**
 * Type representing fields parsed from a commit message.
 * Internally, this includes summary and test plan, etc.
 * Externally, this is just the description right now
 */
export const CommitMessageFieldUtils: CommitMessageFieldsUtilsType =
  Internal.CommitMessageFieldUtils ?? OSSCommitMessageFieldsUtils;

function arraysEqual<T>(a: Array<T>, b: Array<T>): boolean {
  if (a.length !== b.length) {
    return false;
  }
  return a.every((val, i) => b[i] === val);
}
