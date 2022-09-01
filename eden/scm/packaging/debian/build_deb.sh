#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

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
