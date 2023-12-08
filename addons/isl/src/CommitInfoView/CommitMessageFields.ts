/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EditedMessage} from './CommitInfoState';
import type {CommitMessageFields, FieldConfig, FieldsBeingEdited} from './types';

import {temporaryCommitTitle} from '../CommitTitle';
import {Internal} from '../Internal';
import {clearOnCwdChange} from '../recoilUtils';
import {atom} from 'recoil';
import {notEmpty} from 'shared/utils';

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

function fieldEqual(
  config: FieldConfig,
  a: Partial<CommitMessageFields>,
  b: Partial<CommitMessageFields>,
): boolean {
  return config.type === 'field'
    ? arraysEqual((a[config.key] ?? []) as Array<string>, (b[config.key] ?? []) as Array<string>)
    : a[config.key] === b[config.key];
}

/**
 * Construct value representing which fields differ between two parsed messages, by comparing each field.
 * ```
 * findFieldsBeingEdited({title: 'hi', description: 'yo'}, {title: 'hey', description: 'yo'}) -> {title: true, description: false}
 * ```
 */
export function findFieldsBeingEdited(
  schema: Array<FieldConfig>,
  a: Partial<CommitMessageFields>,
  b: Partial<CommitMessageFields>,
): FieldsBeingEdited {
  return Object.fromEntries(schema.map(config => [config.key, !fieldEqual(config, a, b)]));
}

export function anyEditsMade(
  schema: Array<FieldConfig>,
  latestMessage: CommitMessageFields,
  edited: Partial<CommitMessageFields>,
): boolean {
  return Object.keys(edited).some(key => {
    const config = schema.find(config => config.key === key);
    if (config == null) {
      return false;
    }
    return !fieldEqual(config, latestMessage, edited);
  });
}

/** Given an edited message (Partial<CommitMessageFields>), remove any fields that haven't been meaningfully edited.
 * (exactly equals latest underlying message)
 */
export function removeNoopEdits(
  schema: Array<FieldConfig>,
  latestMessage: CommitMessageFields,
  edited: Partial<CommitMessageFields>,
): Partial<CommitMessageFields> {
  return Object.fromEntries(
    Object.entries(edited).filter(([key]) => {
      const config = schema.find(config => config.key === key);
      if (config == null) {
        return false;
      }
      return !fieldEqual(config, latestMessage, edited);
    }),
  );
}

function isFieldNonEmpty(field: string | Array<string>) {
  return Array.isArray(field) ? field.length > 0 : field && field.trim().length > 0;
}

export function commitMessageFieldsToString(
  schema: Array<FieldConfig>,
  fields: CommitMessageFields,
  allowEmptyTitle?: boolean,
): string {
  return schema
    .filter(config => config.key === 'Title' || isFieldNonEmpty(fields[config.key]))
    .map(config => {
      // stringified messages of the form Key: value, except the title or generic description don't need a label
      const prefix =
        config.key === 'Title' || config.key === 'Description' ? '' : config.key + ': ';

      if (config.key === 'Title') {
        const value = fields[config.key] as string;
        if (allowEmptyTitle !== true && value.trim().length === 0) {
          return temporaryCommitTitle();
        }
      }

      const value =
        config.type === 'field'
          ? (config.formatValues ?? joinWithComma)(fields[config.key] as Array<string>)
          : fields[config.key];
      return prefix + value;
    })
    .join('\n\n');
}

export function mergeCommitMessageFields(
  schema: Array<FieldConfig>,
  a: CommitMessageFields,
  b: CommitMessageFields,
): CommitMessageFields {
  return Object.fromEntries(
    schema
      .map(config => {
        const isANonEmpty = isFieldNonEmpty(a[config.key]);
        const isBNonEmpty = isFieldNonEmpty(b[config.key]);
        if (!isANonEmpty && !isBNonEmpty) {
          return undefined;
        } else if (!isANonEmpty || !isBNonEmpty) {
          return [config.key, isANonEmpty ? a[config.key] : b[config.key]];
        } else if (Array.isArray(a[config.key])) {
          const av = a[config.key] as Array<string>;
          const bv = b[config.key] as Array<string>;
          const merged = arraysEqual(av, bv) ? av : [...av, ...bv];
          return [
            config.key,
            config.type === 'field' && config.maxTokens != null
              ? merged.slice(0, config.maxTokens)
              : merged,
          ];
        } else {
          const av = a[config.key] as string;
          const bv = b[config.key] as string;
          const merged =
            av.trim() === bv.trim() ? av : av + (config.type === 'title' ? ', ' : '\n') + bv;
          return [config.key, merged];
        }
      })
      .filter(notEmpty),
  );
}

export function mergeManyCommitMessageFields(
  schema: Array<FieldConfig>,
  fields: Array<CommitMessageFields>,
): CommitMessageFields {
  return Object.fromEntries(
    schema
      .map(config => {
        if (Array.isArray(fields[0][config.key])) {
          return [
            config.key,
            [...new Set(fields.flatMap(field => field[config.key]))].slice(
              0,
              (config.type === 'field' ? config.maxTokens : undefined) ?? Infinity,
            ),
          ];
        } else {
          const result = fields
            .map(field => field[config.key])
            .filter(value => ((value as string | undefined)?.trim().length ?? 0) > 0);
          if (result.length === 0) {
            return undefined;
          }
          return [config.key, result.join(config.type === 'title' ? ', ' : '\n')];
        }
      })
      .filter(notEmpty),
  );
}

function joinWithComma(tokens: Array<string>): string {
  return tokens.join(', ');
}

/**
 * Look through the message fields for a diff number
 */
export function findEditedDiffNumber(field: CommitMessageFields): string | undefined {
  const found = field['Differential Revision'];
  if (Array.isArray(found)) {
    return found[0];
  }
  return found;
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
  title: string, // TODO: remove title and just pass title\ndescription in one thing
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

export function editedMessageSubset(
  message: CommitMessageFields,
  fieldsBeingEdited: FieldsBeingEdited,
): EditedMessage {
  const fields = Object.fromEntries(
    Object.entries(message).filter(([k]) => fieldsBeingEdited[k] ?? false),
  );
  return {fields};
}

export function applyEditedFields(
  message: CommitMessageFields,
  editedMessage: Partial<CommitMessageFields>,
): CommitMessageFields {
  return {...message, ...editedMessage} as CommitMessageFields;
}
