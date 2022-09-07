#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Verifies a Sapling .deb on Ubuntu 20.04 or 22.04.
#
# This script takes the URL to a .deb from the Releases page such as:
#
#     https://github.com/bolinfest/eden/releases/download/20220831-124726-b53cb8ea/sapling_0.0-20220831.124726.b53cb8ea_amd64.Ubuntu20.04.deb
#
# and creates a barebones Docker container to install and run it.
# Note that a sapling.deb should not assume the user has `apt install`'d
# anything else.

set -e
set -x

URL="$1"
echo "$URL"

tmp_dir=$(mktemp -d -t ci-XXXXXXXXXX)
echo "$tmp_dir"
cd "$tmp_dir"

if [[ "$URL" == *"Ubuntu20.04.deb" ]]; then
  UBUNTU_VERSION=20.04
else
  if [[ "$URL" == *"Ubuntu22.04.deb" ]]; then
    UBUNTU_VERSION=22.04
  else
    echo "could not determine Ubuntu version from URL '${URL}'" >> /dev/stderr
    exit 1
  fi
fi

curl --location -O "$URL"

cat > Dockerfile <<EOF
FROM ubuntu:${UBUNTU_VERSION}
COPY *.deb /root/sapling.deb
RUN apt update -y
RUN apt install -y /root/sapling.deb
RUN which sl
RUN sl clone --git https://github.com/bolinfest/opensnoop-native
EOF

IMAGE_ID=$(echo $RANDOM | shasum | head -c 8)
IMAGE_NAME="sapling-test-${IMAGE_ID}"
docker build -t "$IMAGE_NAME" .
docker run -it "$IMAGE_NAME" /bin/bash
