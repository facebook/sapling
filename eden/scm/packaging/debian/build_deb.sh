#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

# Determine the version of Ubuntu by sourcing /etc/os-release. While there is
# debate about whether source'ing the file creates a security issue:
#
# https://unix.stackexchange.com/questions/432816/grab-id-of-os-from-etc-os-release
#
# It is worth noting that this is how lsb_release gets its data from /etc/lsb-release:
#
# https://github.com/OpenMandrivaSoftware/lsb-release/blob/fd23738b8411/lsb_release#L194
if [ -f /etc/os-release ]; then
  UBUNTU_VERSION=$(bash -c 'source /etc/os-release; echo $VERSION_ID')
else
  echo "could not find /etc/os-release" >> /dev/stderr
  exit 1
fi

# Values of GIT_DEB_DEP below are based on running the following on "pristine"
# Docker containers for the various versions of Ubuntu:
#
# apt update -y
# apt install -y git
# apt list --installed | grep -E '^git/'
case "$UBUNTU_VERSION" in
  20.04)
    # For Ubuntu 20.04, target Python 3.8.
    export PY_VERSION=38
    GIT_DEB_DEP="git (>= 1:2.25.1)"
    ;;

  22.04)
    # For Ubuntu 22.04, target Python 3.10.
    export PY_VERSION=310
    GIT_DEB_DEP="git (>= 1:2.34.1)"
    ;;

  *)
    echo "unsupported Ubuntu version: '${UBUNTU_VERSION}'" >> /dev/stderr
    exit 1
    ;;
esac

DESTDIR=install make PREFIX=/usr install-oss

# For simplicity, we currently use `dpkg-deb --build`, though we should
# ultimately migrate to dpkg-buildpackage. Because we are going to mess with
# the contents of the install folder, we create a copy to work with instead.
pkg_dir=$(mktemp -d -t sapling-XXXXXXXX)
cp --recursive install "$pkg_dir"
mkdir "${pkg_dir}/install/debian"
cp packaging/debian/control "${pkg_dir}/install/debian/control"

# Ultimately, we will add configuration to prevent setup.py from producing
# this hg file.
pushd "$pkg_dir"
rm install/usr/bin/hg

# dpkg-shlibdeps requires the file `debian/control` to exist in the folder
# in which it is run.
pushd install
DEB_DEPS=$(dpkg-shlibdeps -O -e usr/bin/*)
# dpkg-shlibdeps does not know about the runtime dependency on Git,
# so it must be added explicitly.
DEB_DEPS="${DEB_DEPS}, ${GIT_DEB_DEP}"
popd

# In contrast to dpkg-shlibdeps, dpkg-deb requires the file to be named
# `DEBIAN/control`, so we rename the directory and proceed.
mv install/debian install/DEBIAN
echo "$DEB_DEPS" | sed -e 's/shlibs:Depends=/Depends: /' >> install/DEBIAN/control
sed -i "s/%VERSION%/$VERSION/g" install/DEBIAN/control

dpkg-deb --build --root-owner-group install

popd
cp "${pkg_dir}/install.deb" .
dpkg-name install.deb
