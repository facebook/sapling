# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test the flamegraph endpoint for memory profiling

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

# Setup a simple git repository
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "test content" > test.txt
  $ git add test.txt
  $ git commit -qam "Initial commit"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN" "$GIT_REPO"
  Cloning into '$TESTTMP/repo-git'...
  done.

# Import to Mononoke
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Enable jemalloc profiling and start the Mononoke Git Service with --skip-authorization
  $ export MALLOC_CONF=prof:true,prof_active:true
  $ mononoke_git_service --skip-authorization

# Test that flamegraph endpoint returns 200 and SVG content when profiling is enabled
  $ sslcurl -s -o "$TESTTMP/flamegraph.svg" -w "%{http_code}" "https://localhost:$MONONOKE_GIT_SERVICE_PORT/flamegraph"
  200 (no-eol)

# Verify the response starts with XML/SVG header
  $ head -c 100 "$TESTTMP/flamegraph.svg"
  <?xml version="1.0" standalone="no"?><!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.o (no-eol)

# Verify the response contains SVG elements
  $ grep -q "<svg" "$TESTTMP/flamegraph.svg" && echo "SVG element found"
  SVG element found

# Verify health check still works
  $ sslcurl -s "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check"
  I_AM_ALIVE
