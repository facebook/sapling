/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/** Values for each field key,  */
export type CommitMessageFields = Record<string, string | Array<string>>;

/**
 * Which fields of the message should display as editors instead of rendered values.
 * This can be controlled outside of the commit info view, but it gets updated in an effect as well when commits are changed.
 * `forceWhileOnHead` can be used to prevent auto-updating when in amend mode to bypass this effect.
 * This value is removed whenever the next real update to the value is given.
 *
 * ```
 * {
 *   title: boolean,
 *   description: boolean,
 *   ...
 * }
 * ```
 */
export type FieldsBeingEdited = Record<string, boolean> & {forceWhileOnHead?: boolean};

export interface CommitMessageFieldsUtilsType {
  /**
   * Extract fields from string commit message
   */
  parseCommitMessageFields: (title: string, description: string) => CommitMessageFields;
  /**
   * Convert fields back into a string commit message, the opposite of parseCommitMessageFields.
   */
  commitMessageFieldsToString: (fields: CommitMessageFields) => string;

  /**
   * Schema for fields in a commit message
   */
  configuredFields: Array<FieldConfig>;

  /**
   * Construct value representing all fields are false: {title: false, description: false, ...}
   */
  noFieldsBeingEdited: () => FieldsBeingEdited;
  /**
   * Construct value representing all fields are being edited: {title: true, description: true, ...}
   */
  allFieldsBeingEdited: () => FieldsBeingEdited;
  /**
   * Construct value representing which fields differ between two parsed messages, by comparing each field.
   * ```
   * findFieldsBeingEdited({title: 'hi', description: 'yo'}, {title: 'hey', description: 'yo'}) -> {title: true, description: false}
   * ```
   */
  findFieldsBeingEdited: (a: CommitMessageFields, b: CommitMessageFields) => FieldsBeingEdited;
}

/**
 * Dynamic configuration for a single field in a commit message
 */
export type FieldConfig = {
  /** i18n key for the display name for this field. Note: this should be provided to t() or <T> to render. */
  name: string;
  /**
   * Internal label for this field, unrelated to how it was parsed from the message.
   * commitMessageFieldsToString handles re-inserting parseable tags.
   */
  key: 'title' | string;
  /** Codicon to show next to this field */
  icon: string;
} & (
  | {
      /**
       * Type of the field to show in the UI.
       * textarea => long form content, with extra buttons for image uploading, etc. Supports vertical resize.
       * field => single-line, tokenized field
       * title => non-resizeable textarea for the title, which has special rendering.
       */
      type: 'title' | 'textarea';
    }
  | {
      type: 'field';
      autocompleteType: 'user' | 'task';
    }
);
