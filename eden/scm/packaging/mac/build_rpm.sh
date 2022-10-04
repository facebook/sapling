#!/bin/zsh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

PYTHON_VERSION=3.8
PYTHON_VERSION_SHORT=38

ROOT=$(cd ../.. && pwd)

rpmbuild --target i386 \
    --define "py_version $PYTHON_VERSION" \
    --define "_bindir /opt/sapling-beta/bin" \
    --define "_libdir /opt/sapling-beta/lib" \
    --define "_sysconfdir /etc" \
    --define "_datadir /opt/sapling-beta/share" \
    --define "_docdir /opt/sapling-beta/share/doc" \
    --define "extra_setup_py_args --prefix=/opt/sapling-beta" \
    --define "_prefix /opt/sapling-beta" \
    --define "python_prefix /opt/sapling-beta" \
    --define "python_sitepackage /opt/sapling-beta/lib/python$PYTHON_VERSION/site-packages" \
    --define "python_sitelib /opt/sapling-beta/lib/python$PYTHON_VERSION/site-packages" \
    --define "python_sitearch /opt/sapling-beta/lib/python$PYTHON_VERSION/site-packages" \
    --define "__python /opt/homebrew/opt/python$PYTHON_VERSION_SHORT/bin/python$PYTHON_VERSION" \
    --define "_tmppath /var/tmp" \
    --define "version $VERSION" \
    --define "sapling_root $ROOT" \
    -v -bb sapling.spec
