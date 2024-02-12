/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * Runs the Webpack build and post-processes the build/ folder so that it can
 * be deployed on GitHub Pages.
 *
 * This script is expected to be run from the project root.
 */

const child_process = require('child_process');
const fs = require('fs');
const path = require('path');

const moduleRoot = __dirname;

function rm_rf(file) {
  fs.rmSync(file, {force: true, recursive: true});
}

const buildFolder = path.join(moduleRoot, 'build');

rm_rf(buildFolder);
child_process.execSync('yarn run build');

function replaceDevelopmentJavaScriptURLsWithProductionURLs(htmlFile) {
  // In development, we fetch third-party dependencies from
  // https://cdn.jsdelivr.net so that Webpack does not have to spend cycles
  // packaging them. In production, we still use separate <script> tags for
  // these dependencies, but we include them so we are not dependent on a CDN.
  //
  // Key is the value in public/index.html; value is the path within the
  // corresponding Node module.
  const scriptURLMapping = {
    'react@18.2.0/umd/react.development.min.js': 'react/umd/react.production.min.js',
    'react-dom@18.2.0/umd/react-dom.development.min.js':
      'react-dom/umd/react-dom.production.min.js',
    'recoil@0.7.5/umd/index.js': 'recoil/umd/index.min.js',
    'history@5.3.0/umd/history.production.min.js': 'history/umd/history.production.min.js',
    'react-router@6.2.2/umd/react-router.development.js':
      'react-router/umd/react-router.production.min.js',
    'react-router-dom@6.2.2/umd/react-router-dom.development.min.js':
      'react-router-dom/umd/react-router-dom.production.min.js',
  };

  const nodeModules = path.resolve(path.join(moduleRoot, '..', 'node_modules'));

  function replaceScriptSrc(html, oldURL, pathWithinNodeModules) {
    const fullOldURL = `https://cdn.jsdelivr.net/npm/${oldURL}`;
    const expected = `<script src="${fullOldURL}"></script>`;
    if (html.indexOf(expected) === -1) {
      throw Error(`could not find '${expected}' in HTML`);
    }

    const match = oldURL.match(/^([a-z-]+)@([^/]+)\/.*/);
    const moduleName = match[1];
    const version = match[2];
    const manifestPath = path.join(nodeModules, moduleName, 'package.json');
    const manifest = JSON.parse(
      fs.readFileSync(manifestPath, {
        encoding: 'utf8',
      }),
    );
    if (manifest.version !== version) {
      throw Error(
        `ERROR: version mismatch for ${moduleName}: expected ${version} but found ${manifest.version}.` +
          'Verify new version works and update build script, if appropriate.',
      );
    }

    const productionPath = `static/js/${moduleName}@${version}-production.js`;
    const nodeModulesPath = path.join(nodeModules, pathWithinNodeModules);
    fs.copyFileSync(nodeModulesPath, path.join(buildFolder, productionPath));

    if (moduleHasSourceMap(moduleName)) {
      // Copy sourcemap for third-party dependency. For the prod JS, we include
      // the version number in the URL so it can be cached. We don't worry about
      // this for third-party sourcemaps, which also avoids the need to rewrite
      // the `//# sourceMappingURL` comment to point to a different path.
      fs.copyFileSync(
        `${nodeModulesPath}.map`,
        path.join(buildFolder, 'static', 'js', `${path.basename(nodeModulesPath)}.map`),
      );
    }

    const replacement = `<script src="/${productionPath}"></script>`;

    return html.replace(expected, replacement);
  }

  let html = fs.readFileSync(htmlFile, {encoding: 'utf8'});
  for (const [oldURL, newURL] of Object.entries(scriptURLMapping)) {
    html = replaceScriptSrc(html, oldURL, newURL);
  }
  fs.writeFileSync(indexHtmlFile, html);
}

function moduleHasSourceMap(moduleName) {
  // Unclear why react, react-dom, and recoil do not ship with source maps.
  // history has one, but it does not appear to have real data.
  return moduleName === 'react-router' || moduleName === 'react-router-dom';
}

const indexHtmlFile = path.join(buildFolder, 'index.html');
replaceDevelopmentJavaScriptURLsWithProductionURLs(indexHtmlFile);
fs.copyFileSync(indexHtmlFile, path.join(buildFolder, '404.html'));
