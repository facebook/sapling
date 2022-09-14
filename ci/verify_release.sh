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
#
# If the file has already been download locally (perhaps from a private
# GitHub repo?), then the local path can also be used as an argument:
#
#     verify_release.sh ~/Downloads/sapling_0.0-20220831.124726.b53cb8ea_amd64.Ubuntu20.04.deb
#
# Note that a sapling.deb should not assume the user has `apt install`'d
# anything else.

set -e
set -x

FILE_OR_URL="$1"
echo "$FILE_OR_URL"

tmp_dir=$(mktemp -d -t ci-XXXXXXXXXX)
echo "$tmp_dir"
cd "$tmp_dir"

if [[ "$FILE_OR_URL" == *"Ubuntu20.04.deb" ]]; then
  UBUNTU_VERSION=20.04
else
  if [[ "$FILE_OR_URL" == *"Ubuntu22.04.deb" ]]; then
    UBUNTU_VERSION=22.04
  else
    echo "could not determine Ubuntu version from arg '${FILE_OR_URL}'" >> /dev/stderr
    exit 1
  fi
fi

if [ ! -f "$FILE_OR_URL" ]; then
  curl --location -O "$FILE_OR_URL"
else
  cp "$FILE_OR_URL" .
fi

cat > Dockerfile <<EOF
FROM ubuntu:${UBUNTU_VERSION}
COPY *.deb /root/sapling.deb

# https://serverfault.com/a/1016972 to ensure installing tzdata does not
# result in a prompt that hangs forever.
ARG DEBIAN_FRONTEND=noninteractive
ENV TZ=Etc/UTC

RUN apt update -y
RUN apt install -y /root/sapling.deb
RUN which sl
RUN sl clone --git https://github.com/bolinfest/opensnoop-native
EOF

IMAGE_ID=$(echo $RANDOM | shasum | head -c 8)
IMAGE_NAME="sapling-test-${IMAGE_ID}"
docker build -t "$IMAGE_NAME" .
docker run -it "$IMAGE_NAME" /bin/bash
