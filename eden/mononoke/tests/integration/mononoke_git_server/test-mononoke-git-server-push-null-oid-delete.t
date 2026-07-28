# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Set Mononoke as the Source of Truth, otherwise the push is rejected by the
# source-of-truth gate and the "ng" below would be for the wrong reason.
  $ set_mononoke_as_source_of_truth_for_git

# Craft a raw receive-pack request that "deletes" refs/heads/foo with BOTH the old
# and new oid set to the null (all-zeros) sha1 -- an idempotent delete of a ref that
# does not exist. Real `git` never emits this (it refuses to delete a ref the server
# does not advertise), so we hand-build the git wire-protocol packetlines, the same
# technique as test-mononoke-git-server-push-empty-repo-with-deltas.t.
#
# The pkt-line length prefix counts its own 4 bytes. The command line
#   "<40 zeros> <40 zeros> refs/heads/foo\0 report-status quiet object-format=sha1"
# is 136 bytes -> +4 = 140 = 0x8c -> "008c", followed by a flush-pkt "0000".
# A delete-only push carries no packfile.
  $ ZERO="0000000000000000000000000000000000000000"
  $ capabilities="report-status quiet object-format=sha1"
  $ printf "008c$ZERO $ZERO refs/heads/foo\0 $capabilities" > push_data
  $ echo -n "0000" >> push_data

# POST it to the receive-pack endpoint and capture the HTTP status.
  $ curl -X POST $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git/git-receive-pack \
  >   -H 'Content-Type: application/x-git-receive-pack-request' \
  >   -H 'Accept: application/x-git-receive-pack-result' \
  >   -k --cert "$TEST_CERTDIR/client0.crt" --key "$TEST_CERTDIR/client0.key" \
  >   --data-binary "@push_data" -o "$TESTTMP/resp.bin" -s -w "Response code: %{http_code}\n"
  Response code: 200

# The push must NOT 500: the (empty) pack is accepted ...
  $ grep -qa 'unpack ok' "$TESTTMP/resp.bin" && echo "pack accepted (not 5xx)"
  pack accepted (not 5xx)

# ... and the client gets the downstream validation error for the degenerate 0/0 ref.
  $ grep -ao 'ng refs/heads/foo.*' "$TESTTMP/resp.bin"
  ng refs/heads/foo Invalid bookmark operation. Both old and new changesets cannot be None
