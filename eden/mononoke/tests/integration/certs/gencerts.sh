#!/usr/bin/env bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -x

if [[ -z "$1" ]]; then
    echo "Provide path to openssl.cnf file"
    exit 1
fi

# Generate private key for root certificate
openssl genrsa -out root-ca.key 2048
# Generate root certificate
openssl req -x509 -new -nodes -key root-ca.key -subj "/C=US/ST=CA/O=TestRoot/CN=mononoke.com" -sha256 -days 3650 -out root-ca.crt
# Generate private key for server/client certificate
# (could be separate as well, but keep them as one for ease)
openssl genrsa -out localhost.key 2048
# Generate certificate signing request for server/client certificate
openssl req -new -sha256 -key localhost.key -config "$1" -out localhost.csr

# Sign server/client certificate with root certificate
if grep -q '\[ v3_ca \]' "$1"; then
  # ...and apply custom v3_ca extensions
  openssl x509 -extfile "$1" -extensions v3_ca -req -in localhost.csr -CA root-ca.crt -CAkey root-ca.key -CAcreateserial -out localhost.crt -days 3650 -sha256
else
  openssl x509 -req -in localhost.csr -CA root-ca.crt -CAkey root-ca.key -CAcreateserial -out localhost.crt -days 3650 -sha256
fi
