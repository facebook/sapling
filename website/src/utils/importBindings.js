/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Mutex} from 'async-mutex';

let cached;
const mutex = new Mutex();

// Imports the `bindings` wasm module.
//
// Designed for `wasm-pack -t web` build.
// See `src/wasm-bindings/build.sh` for context.
//
// Ideally we can use `wasm-pack -t bundler` build and regular
// `import`s at the top of a module. However it has a few issues:
// - `yarn build` fails during SSR rendering. Something like
//   `ReferenceError: __dirname is not defined`. The trace is too
//   cryptic (like `at Object.1051 (main:278707:21)`) to debug.
//   This can be worked around by using `import()` in a function
//   body.
// - Some webservers (ex. interndocs) do not use `application/wasm`
//   Content-Type. Browsers will refuse to compile wasm.
//   The workaround is similar to the code produced by `-t web` build.
export default async function importBindings() {
  return await mutex.runExclusive(async () => {
    if (cached) {
      return cached;
    }
    const bindings = await import("@site/static/wasm/wasm_bindings.js");
    await bindings.default();
    cached = bindings;
    if (process.env.NODE_ENV === 'development') {
      if (window) {
        window.B = bindings;
      }
    }
    return bindings;
  });
};
