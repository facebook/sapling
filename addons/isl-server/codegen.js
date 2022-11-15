/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const child_process = require('child_process');

child_process.execSync('yarn graphql-codegen --config codegen.github.yml', {stdio: 'inherit'});

// @fb-only
