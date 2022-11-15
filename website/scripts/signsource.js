/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const fs = require('fs');
// eslint-disable-next-line import/no-extraneous-dependencies
const signedsource = require('signedsource');

const filenames = process.argv.slice(2);
if (filenames.length === 0) {
  // eslint-disable-next-line no-console
  console.info('must specify at least one file to sign');
  process.exit(1);
}

filenames.forEach((filename) => {
  const contents = fs.readFileSync(filename, {encoding: 'utf8'});
  const signedContents = signedsource.signFile(contents);
  fs.writeFileSync(filename, signedContents);
});
