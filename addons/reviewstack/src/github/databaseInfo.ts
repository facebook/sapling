/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * As of Nov 16, 2022, Firefox does not support indexedDB.databases():
 * https://bugzilla.mozilla.org/show_bug.cgi?id=934640.
 * To support deleting all databases, we have to keep a list of every
 * IndexedDB database that was *ever* created by ReviewStack so that we can
 * be sure to delete all of them during login/logout.
 */
export const ALL_DB_NAMES_EVER: ReadonlyArray<string> = ['github-objects-v2'];

/** Update ALL_DB_NAMES_EVER as appropriate, if DB_VERSION changes. */
export const DB_VERSION = 2;
/** Update ALL_DB_NAMES_EVER as appropriate, if DB_NAME changes. */
export const DB_NAME = `github-objects-v${DB_VERSION}`;
