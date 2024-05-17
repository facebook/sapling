# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is root level README file. this is root level README file. this is root level README file" > README.md
  $ echo "assume that this is root level bootstrap script. assume that this is root level bootstrap script. assume that this is root level bootstrap script" > bootstrap.sh
  $ echo "this is the license file, everyone can use this code. this is the license file, everyone can use this code. this is the license file, everyone can use this code" > LICENSE.md  
  $ git add .
  $ git commit -qam "Added root level information"
  $ git tag -a -m "new tag" first_tag

  $ mkdir -p client/android
  $ mkdir -p client/ios
  $ echo "this is the client directory that contains android and ios code. this is the client directory that contains android and ios code" > client/README.md
  $ echo "android source code file for main. android source code file for main. android source code file for main. android source code file for main" > client/android/main.java
  $ echo "ios source code file for main. ios source code file for main. ios source code file for main. ios source code file for main." > client/ios/main.swift
  $ git add .
  $ git commit -qam "Added client side files"
  $ git tag -a -m "client code tag" client_tag

  $ mkdir -p service/common
  $ mkdir -p service/routing
  $ mkdir -p service/handlers
  $ echo "this is backend routing code. this is backend routing code. this is backend routing code." > service/routing/route.rs
  $ echo "this is controller handler code. this is controller handler code. this is controller handler code." > service/handlers/main.rs
  $ echo "this is common library used across the service crates. this is common library used across the service crates. this is common library used across the service crates." > service/common/lib.rs
  $ git add .
  $ git commit -qam "Added server side files"
  $ git tag -a -m "server code tag" server_tag

  $ mkdir -p web/browser
  $ mkdir -p web/stylesheets
  $ echo "this is the web directory that contains JavaScript code. this is the web directory that contains JavaScript code. this is the web directory that contains JavaScript code" > web/README.md
  $ echo "this is the website code in JS. this is the website code in JS. this is the website code in JS" > web/browser/script.js
  $ echo "this is the style sheets stuff. this is the style sheets stuff. this is the style sheets stuff" > web/stylesheets/design.css
  $ git add .
  $ git commit -qam "Added website related files"
  $ git tag -a -m "website code tag" website_tag

  $ touch sparse_profile
  $ cat > sparse_profile <<EOF
  > /*
  > !/*/
  > /client/
  > !/client/*/
  > /client/android/
  > EOF
  $ git add .
  $ git commit -qam "Added sparse checkout file in repo"

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform a sparse clone of the repo with server side sparse filtering
  $ cd "$TESTTMP"
  $ git clone --filter=sparse:oid=master:sparse_profile --no-checkout file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
# Validate that we got a subset of objects
  $ git rev-list --all --quiet --objects --missing=print | wc -l
  7

# Attempt to perform a sparse clone of the repo with server side sparse filtering in Mononoke
# Since Mononoke doesn't support server-side sparse filtering, it will treat it as regular clone and return
# full-clone objects
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --filter=sparse:oid=master:sparse_profile --no-checkout
  Cloning into 'repo'...
  $ cd $REPONAME
# Validate that we aren't missing any objects since the server gave us a full clone
  $ git rev-list --all --quiet --objects --missing=print | wc -l
  0
