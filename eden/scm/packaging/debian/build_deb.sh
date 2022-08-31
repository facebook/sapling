#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

# dpkg-shlibdeps requires the file `debian/control` to exist in the folder
# in which it is run.
mkdir -p install/debian
cp packaging/debian/control install/debian
cd install
DEB_DEPS=$(dpkg-shlibdeps -O -e usr/local/bin/*)
# dpkg-shlibdeps does not know about the runtime dependency on Git,
# so it must be added explicitly.
DEB_DEPS="${DEB_DEPS}, ${GIT_DEB_DEP}"
cd ..

# In contrast to dpkg-shlibdeps, dpkg-deb requires the file to be named
# `DEBIAN/control`, so we rename the directory and proceed.
mv install/debian install/DEBIAN
echo "$DEB_DEPS" | sed -e 's/shlibs:Depends=/Depends: /' >> install/DEBIAN/control
sed -i "s/%VERSION%/$VERSION/g" install/DEBIAN/control

dpkg-deb --build --root-owner-group install
dpkg-name install.deb
