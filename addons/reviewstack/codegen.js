/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * Performs any codegen steps that are necessary for development.
 *
 * This script is expected to be run from the project root.
 */

const child_process = require('child_process');

child_process.execSync('yarn run graphql');
child_process.execSync('yarn run textmate');
