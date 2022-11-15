/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import LanguageExtensionOnGitHub from './LanguageExtensionOnGitHub';

/**
 * Commit within the https://github.com/Microsoft/vscode/ repo to use when
 * fetching TextMate grammars from extensions bundled with VS Code.
 */
const VSCODE_COMMIT = 'dfd34e8260c270da74b5c2d86d61aee4b6d56977'; // tag 1.66.2

const extensions = [
  // List of extensions in the VS Code repository that we look through to find
  // language definitions and their associated TextMate grammars.
  'extensions/bat/',
  'extensions/clojure/',
  'extensions/coffeescript/',
  'extensions/cpp/',
  'extensions/csharp/',
  'extensions/css/',
  'extensions/dart',
  'extensions/docker/',
  'extensions/fsharp/',
  'extensions/git-base/',
  'extensions/go/',
  'extensions/groovy/',
  'extensions/handlebars/',
  'extensions/hlsl/',
  'extensions/html/',
  'extensions/ini/',
  'extensions/java/',
  'extensions/javascript/',
  // TODO(T135888354): Multiple extensions are specifying a `configuration` for jsonc?
  // 'extensions/json/',
  'extensions/julia/',
  'extensions/less/',
  'extensions/log/',
  'extensions/lua/',
  'extensions/make/',
  'extensions/markdown-basics/',
  'extensions/objective-c/',
  'extensions/perl/',
  // Deliberately excluding extensions/php/ so that .php files are associated
  // with the Hack grammar.
  'extensions/powershell/',
  'extensions/pug/',
  'extensions/python/',
  'extensions/r/',
  'extensions/razor/',
  'extensions/ruby/',
  'extensions/rust/',
  'extensions/scss/',
  'extensions/shellscript/',
  'extensions/sql/',
  'extensions/swift/',
  'extensions/typescript-basics/',
  'extensions/vb/',
  'extensions/xml/',
  'extensions/yaml/',
].map(
  path =>
    new LanguageExtensionOnGitHub({
      organization: 'microsoft',
      project: 'vscode',
      commit: VSCODE_COMMIT,
      path,
    }),
);

// GDScript
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'godotengine',
    project: 'godot-vscode-plugin',
    commit: 'd404eaedc6fd04f1c36e1ec397f1bc1500015780', // 1.2.0
  }),
);

// Hack
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'slackhq',
    project: 'vscode-hack',
    commit: '4b8dc3e067e09932e346fe7dad49b8a6f8f88d6f', // v2.13.0
  }),
);

// Haskell
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'JustusAdam',
    project: 'language-haskell',
    commit: '98a8f3ae06ab9f5bde015b48ea7eed47dbc4c9aa', // v3.4.0
  }),
);

// Kotlin
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'mathiasfrohlich',
    project: 'vscode-kotlin',
    commit: '090fe4cd054d6142d7eaefdb69c12d4b063a089e',
  }),
);

// Thrift
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'MrKou47',
    project: 'thrift-syntax-support',
    commit: '425ab2a21eabb54d26a7fb23d1bd1a67067c6ae2',
  }),
);

// TOML
extensions.push(
  new LanguageExtensionOnGitHub({
    organization: 'bungcip',
    project: 'better-toml',
    commit: '34caa1c12a3a70501729727adf291bd9a56755d6',
  }),
);

export default extensions;
