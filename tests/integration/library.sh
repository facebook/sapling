#!/bin/bash
# Library routines and initial setup for Mononoke-related tests.

function mononoke {
  $MONONOKE_SERVER --repotype revlog "$@" >> "$TESTTMP/mononoke.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

function blobimport {
  $MONONOKE_BLOBIMPORT "$@"
}

function edenserver {
  $MONONOKE_EDEN_SERVER "$@" >> "$TESTTMP/edenserver.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

# Run an hg binary configured with the settings required to talk to Mononoke.
function hgmn {
  hg --config ui.ssh="$DUMMYSSH" --config ui.remotecmd="$MONONOKE_HGCLI" "$@"
}
