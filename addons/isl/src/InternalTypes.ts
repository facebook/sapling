/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @fb-only: import type {InternalTypeImports} from './facebook/InternalTypeImports';

// Note: this file may be imported by the ISL server via `types.ts`, so it should not transitively import any tsx files,
// which is why it's separate from `Internal.ts`.

/**
 * API for accessing types for internal (non-OSS) features / functions.
 * Note: in non-internal builds, this uses Record<string, never> as the replacement type,
 * that is, an empty object. (the type equivalent of Partial<> used for InternalImports)
 */
// prettier-ignore
export type InternalTypes =
 // @fb-only: InternalTypeImports;
// @fb-only: /*
 Record<string, never>;
// @fb-only: */
