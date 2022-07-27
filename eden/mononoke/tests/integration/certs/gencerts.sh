#!/usr/bin/env bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -x

if [[ -z "$1" ]]; then
    echo "Provide names of certificates (e.g. localhost client0)"
    exit 1
fi

if [[ "$1" = "--keep-root" ]]; then
  echo "Using existing root certificate"
  shift
else
  # Generate private key for root certificate
  openssl genrsa -out root-ca.key 2048
  # Generate root certificate
  openssl req -x509 -new -nodes -key root-ca.key -subj "/C=US/ST=CA/O=TestRoot/CN=mononoke.test" -sha256 -days 3650 -out root-ca.crt
fi

# For each certificate name:
for name in "$@"; do
  # Generate private key for certificate
  openssl genrsa -out "$name.key" 2048
  # Generate certificate signing request for server certificate
  openssl req -new -sha256 -key "$name.key" -config "$name-openssl.cnf" -out "$name.csr"

  # Sign certificate with root certificate
  if grep -q '\[ v3_ca \]' "$name-openssl.cnf"; then
    # ...and apply custom v3_ca extensions
    openssl x509 -extfile "$name-openssl.cnf" -extensions v3_ca -req -in "$name.csr" -CA root-ca.crt -CAkey root-ca.key -CAcreateserial -out "$name.crt" -days 3650 -sha256
  else
    openssl x509 -req -in "$name.csr" -CA root-ca.crt -CAkey root-ca.key -CAcreateserial -out "$name.crt" -days 3650 -sha256
  fi
done
