/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitMessageFields, FieldConfig, FieldsBeingEdited} from './types';

import {Internal} from '../Internal';
import {clearOnCwdChange} from '../recoilUtils';
import {atom} from 'recoil';

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

export function commitMessageFieldsToString(
  schema: Array<FieldConfig>,
  fields: CommitMessageFields,
): string {
  return schema
    .filter(config => config.key === 'Title' || fields[config.key])
    .map(
      config =>
        // stringified messages of the form Key: value, except the title or generic description don't need a label
        (config.key === 'Title' || config.key === 'Description' ? '' : config.key + ': ') +
        (config.type === 'field'
          ? (config.formatValues ?? joinWithComma)(fields[config.key] as Array<string>)
          : fields[config.key]),
    )
    .join('\n\n');
}

function joinWithComma(tokens: Array<string>): string {
  return tokens.join(', ');
}

function commaSeparated(s: string | undefined): Array<string> {
  if (s == null || s.trim() === '') {
    return [];
  }
  // TODO: remove duplicates
  const split = s.split(',').map(s => s.trim());
  return split;
}

const SL_COMMIT_MESSAGE_REGEX = /^(HG:.*)|(SL:.*)/gm;

/**
 * Extract fields from string commit message, based on the field schema.
 */
export function parseCommitMessageFields(
  schema: Array<FieldConfig>,
  title: string,
  description: string,
): CommitMessageFields {
  const map: Partial<Record<string, string>> = {};
  const sanitizedCommitMessage = (title + '\n' + description).replace(SL_COMMIT_MESSAGE_REGEX, '');

  const sectionTags = schema.map(field => field.key);
  const TAG_SEPARATOR = ':';
  const sectionSeparatorRegex = new RegExp(`\n\\s*\\b(${sectionTags.join('|')})${TAG_SEPARATOR} ?`);

  // The section names are in a capture group in the regex so the odd elements
  // in the array are the section names.
  const splitSections = sanitizedCommitMessage.split(sectionSeparatorRegex);
  for (let i = 1; i < splitSections.length; i += 2) {
    const sectionTag = splitSections[i];
    const sectionContent = splitSections[i + 1] || '';

    // Special case: If a user types the name of a field in the text, a single section might be
    // discovered more than once.
    if (map[sectionTag]) {
      map[sectionTag] += '\n' + sectionTag + ':\n' + sectionContent.replace(/^\n/, '').trimEnd();
    } else {
      // If we captured the trailing \n in the regex, it could cause leading newlines to not capture.
      // So we instead need to manually trim the leading \n in the content, if it exists.
      map[sectionTag] = sectionContent.replace(/^\n/, '').trimEnd();
    }
  }

  const result = Object.fromEntries(
    schema.map(config => {
      const found = map[config.key] ?? '';
      if (config.key === 'Description') {
        // special case: a field called "description" should contain the entire description,
        // in case you don't have any fields configured.
        // TODO: this should probably be a key on the schema description field instead,
        // or configured as part of the overall schema "parseMethod", to support formats other than "Key: Value"
        return ['Description', description];
      }
      return [
        config.key,
        config.type === 'field' ? (config.extractValues ?? commaSeparated)(found) : found,
      ];
    }),
  );
  // title won't get parsed automatically, manually insert it
  result.Title = title;
  return result;
}

export const OSSDefaultFieldSchema: Array<FieldConfig> = [
  {key: 'Title', type: 'title', icon: 'milestone'},
  {key: 'Description', type: 'textarea', icon: 'note'},
];

function arraysEqual<T>(a: Array<T>, b: Array<T>): boolean {
  if (a.length !== b.length) {
    return false;
  }
  return a.every((val, i) => b[i] === val);
}

/**
 * Schema defining what fields we expect to be in a CommitMessageFields object,
 * and some information about those fields.
 * This is determined by an sl config on the server, hence it lives as an atom.
 */
export const commitMessageFieldsSchema = atom<Array<FieldConfig>>({
  key: 'commitMessageFieldsSchema',
  default: getDefaultCommitMessageSchema(),
  effects: [clearOnCwdChange()],
});

export function getDefaultCommitMessageSchema() {
  return Internal.CommitMessageFieldSchema ?? OSSDefaultFieldSchema;
}
