/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * Runs the build script in the `textmate/` directory and produces two sets of
 * generated files:
 *
 * - `isl/src/generated/textmate/TextMateGrammarManifest.ts` is a
 *   TypeScript source file that is used directly by other TypeScript code in
 *   `isl/src`
 * - A folder of static resources written to `isl/public/generated/textmate`,
 *   which allows ISL to fetch grammers at runtime.
 *
 * This script is expected to be run from the isl/ folder.
 */

const child_process = require('child_process');
const fs = require('fs');

process.chdir(__dirname); // always run relative to isl/

// If no argument is specified, write the static resources to
// `isl/public/generated/textmate`.
const outputFolderArg = process.argv[2];
const grammarsFolder = outputFolderArg ?? './public/generated/textmate';
const textmateModule = '../textmate';

rm_rf(grammarsFolder);
mkdir_p(grammarsFolder);

function rm_rf(path) {
  fs.rmSync(path, {force: true, recursive: true});
}

function mkdir_p(path) {
  fs.mkdirSync(path, {recursive: true});
}

// Clear out the previous build of the textmate module.
rm_rf(`${textmateModule}/dist`);
// Rebuild the textmate module.
child_process.execSync('yarn', {cwd: textmateModule});
child_process.execSync('yarn run tsc', {cwd: textmateModule});

const manifestFolder = 'src/generated/textmate';
rm_rf(manifestFolder);
mkdir_p(manifestFolder);
const manifestPath = `${manifestFolder}/TextMateGrammarManifest.ts`;

const node = 'node --experimental-specifier-resolution=node';
child_process.execSync(`${node} ${textmateModule}/dist/index.js ${manifestPath} ${grammarsFolder}`);

fs.copyFileSync(
  '../node_modules/vscode-oniguruma/release/onig.wasm',
  `${grammarsFolder}/onig.wasm`,
);
