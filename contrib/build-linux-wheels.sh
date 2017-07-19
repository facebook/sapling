#!/bin/bash
# This file is directly inspired by
# https://github.com/pypa/python-manylinux-demo/blob/master/travis/build-wheels.sh
set -e -x

PYTHON_TARGETS=$(ls -d /opt/python/cp27*/bin)

# Create an user for the tests
useradd hgbuilder

# Bypass uid/gid problems
cp -R /src /io && chown -R hgbuilder:hgbuilder /io

# Compile wheels for Python 2.X
for PYBIN in $PYTHON_TARGETS; do
    "${PYBIN}/pip" wheel /io/ -w wheelhouse/
done

# Bundle external shared libraries into the wheels with
# auditwheel (https://github.com/pypa/auditwheel) repair.
# It also fix the ABI tag on the wheel making it pip installable.
for whl in wheelhouse/*.whl; do
    auditwheel repair "$whl" -w /src/wheelhouse/
done

# Install packages and run the tests for all Python versions
cd /io/tests/

for PYBIN in $PYTHON_TARGETS; do
    # Install mercurial wheel as root
    "${PYBIN}/pip" install mercurial --no-index -f /src/wheelhouse
    # But run tests as hgbuilder user (non-root)
    su hgbuilder -c "\"${PYBIN}/python\" /io/tests/run-tests.py --with-hg=\"${PYBIN}/hg\" --blacklist=/io/contrib/linux-wheel-centos5-blacklist"
done
