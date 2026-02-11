# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test LFS redirect handler returns 307 with correct redirect URL
# By default (JustKnob = false), redirects go to dewey-lfs

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

  $ mononoke_git_service

# POST to LFS batch endpoint with prod host header
# Explicit header override: x-route-to-mononoke-git-lfs: 0 forces dewey-lfs
# This makes tests deterministic regardless of JustKnob settings
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.internal.tfbnw.net" \
  >   -H "x-route-to-mononoke-git-lfs: 0" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 https://dewey-lfs.vip.facebook.com/lfs-by-repo/repo/objects/batch (no-eol)

# Same request with corp host header
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.c2p.facebook.net" \
  >   -H "x-route-to-mononoke-git-lfs: 0" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 https://dewey-lfs.vip.facebook.com/lfs-by-repo/repo/objects/batch (no-eol)

# Same request with x2p host header
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.edge.x2p.facebook.net" \
  >   -H "x-route-to-mononoke-git-lfs: 0" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 https://dewey-lfs.vip.facebook.com/lfs-by-repo/repo/objects/batch (no-eol)

# Test with x-route-to-mononoke-git-lfs header override = 1
# Should redirect to mononoke-git-lfs instead of dewey
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.internal.tfbnw.net" \
  >   -H "x-route-to-mononoke-git-lfs: 1" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 https://mononoke-git-lfs.internal.tfbnw.net/repo/objects/batch (no-eol)

# Test header override with corp host
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.c2p.facebook.net" \
  >   -H "x-route-to-mononoke-git-lfs: 1" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 https://mononoke-git-lfs.c2p.facebook.net/repo/objects/batch (no-eol)

# Test header override with x2p host (HTTP, not HTTPS)
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.edge.x2p.facebook.net" \
  >   -H "x-route-to-mononoke-git-lfs: 1" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 http://mononoke-git-lfs.edge.x2p.facebook.net/repo/objects/batch (no-eol)

# Test CorpX2pagent: corp host with corpx2pagent header
# Should use HTTP (not HTTPS) for mononoke-git-lfs
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.c2p.facebook.net" \
  >   -H "corpx2pagent: 1" \
  >   -H "x-route-to-mononoke-git-lfs: 1" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/$REPONAME.git/info/lfs/objects/batch"
  307 http://mononoke-git-lfs.c2p.facebook.net/repo/objects/batch (no-eol)

# Test special characters in repo name: dewey-lfs does NOT URL-encode
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.internal.tfbnw.net" \
  >   -H "x-route-to-mononoke-git-lfs: 0" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/foo/bar.git/info/lfs/objects/batch"
  307 https://dewey-lfs.vip.facebook.com/lfs-by-repo/foo/bar/objects/batch (no-eol)

# Test special characters in repo name: mononoke-git-lfs DOES URL-encode
  $ sslcurl -s -o /dev/null -w "%{http_code} %{redirect_url}" \
  >   -X POST \
  >   -H "Content-Type: application/vnd.git-lfs+json" \
  >   -H "Host: git.internal.tfbnw.net" \
  >   -H "x-route-to-mononoke-git-lfs: 1" \
  >   -d '{}' \
  >   "https://localhost:$MONONOKE_GIT_SERVICE_PORT/repos/git/ro/foo/bar.git/info/lfs/objects/batch"
  307 https://mononoke-git-lfs.internal.tfbnw.net/foo%2Fbar/objects/batch (no-eol)
