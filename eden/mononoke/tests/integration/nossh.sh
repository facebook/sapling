#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

REPONAME="${REPONAME:-"$(hg config | grep remotefilelog.reponame | cut -d "=" -f 2)"}"

CA_PEM=${CA_PEM:-"${TEST_CERTS}/root-ca.crt"}
PRIVATE_KEY=${PRIVATE_KEY:-"${TEST_CERTS}/localhost.key"}
CERT=${CERT:-"${TEST_CERTS}/localhost.crt"}
if [[ $LOCALIP == *":"* ]]; then
  # it is ipv6, surround with brackets
  MONONOKE_PATH="[$LOCALIP]:${MONONOKE_SOCKET}"
else
  MONONOKE_PATH="$LOCALIP:${MONONOKE_SOCKET}"
fi
COMMON_NAME="localhost"

"$MONONOKE_HGCLI" -R "$REPONAME" serve --stdio --mononoke-path "$MONONOKE_PATH" \
  --cert="$CERT" \
  --private-key="$PRIVATE_KEY" \
  --ca-pem="$CA_PEM" \
  --common-name="$COMMON_NAME" \
