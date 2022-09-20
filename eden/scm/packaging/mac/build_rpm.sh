#!/bin/zsh

PYTHON_VERSION=3.8
PYTHON_VERSION_SHORT=38

ROOT=$(cd ../.. && pwd)

rpmbuild --target i386 \
    --define "py_version $PYTHON_VERSION" \
    --define "_bindir /opt/sapling/bin" \
    --define "_libdir /opt/sapling/lib" \
    --define "_sysconfdir /etc" \
    --define "_datadir /opt/sapling/share" \
    --define "_docdir /opt/sapling/share/doc" \
    --define "extra_setup_py_args --prefix=/opt/sapling" \
    --define "_prefix /opt/sapling" \
    --define "python_prefix /opt/sapling" \
    --define "python_sitepackage /opt/sapling/lib/python$PYTHON_VERSION/site-packages" \
    --define "python_sitelib /opt/sapling/lib/python$PYTHON_VERSION/site-packages" \
    --define "python_sitearch /opt/sapling/lib/python$PYTHON_VERSION/site-packages" \
    --define "__python /opt/homebrew/opt/python$PYTHON_VERSION_SHORT/bin/python$PYTHON_VERSION" \
    --define "_tmppath /var/tmp" \
    --define "version $VERSION" \
    --define "sapling_root $ROOT" \
    -v -bb sapling.spec
