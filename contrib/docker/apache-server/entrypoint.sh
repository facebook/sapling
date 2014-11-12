#!/bin/bash

# This script gets executed on container start. Its job is to set up
# the Mercurial environment and invoke the server.

# Mercurial can be started in two modes.
# If the MERCURIAL_SOURCE environment variable is set and it points to a
# Mercurial source directory, we will install Mercurial from that directory.
# Otherwise, we download the Mercurial source and install it manually.

set -e

SOURCE_DIR=/var/hg/source
INSTALL_DIR=/var/hg/install
REPOS_DIR=/var/hg/repos
HTDOCS_DIR=/var/hg/htdocs

if [ ! -d ${SOURCE_DIR} ]; then
  echo "Mercurial source not available at ${SOURCE_DIR}"
  echo "You need to mount a volume containing the Mercurial source code"
  echo "when running the container. For example:"
  echo ""
  echo "  $ docker run -v ~/src/hg:/${SOURCE_DIR} hg-apache"
  echo ""
  echo "This container will now stop running."
  exit 1
fi

echo "Installing Mercurial from ${SOURCE_DIR} into ${INSTALL_DIR}"
pushd ${SOURCE_DIR}
/usr/bin/python2.7 setup.py install --root=/ --prefix=${INSTALL_DIR} --force
popd

mkdir -p ${HTDOCS_DIR}

# Provide a default config if the user hasn't supplied one.
if [ ! -f ${HTDOCS_DIR}/config ]; then
  cp /defaulthgwebconfig ${HTDOCS_DIR}/config
fi

if [ ! -f ${HTDOCS_DIR}/hgweb.wsgi ]; then
  cat >> ${HTDOCS_DIR}/hgweb.wsgi << EOF
config = '${HTDOCS_DIR}/config'

import sys
sys.path.insert(0, '${INSTALL_DIR}/lib/python2.7/site-packages')

from mercurial import demandimport
demandimport.enable()

from mercurial.hgweb import hgweb
application = hgweb(config)
EOF
fi

mkdir -p ${REPOS_DIR}

if [ ! -d ${REPOS_DIR}/repo ]; then
  ${INSTALL_DIR}/bin/hg init ${REPOS_DIR}/repo
  chown -R www-data:www-data ${REPOS_DIR}/repo
fi

# This is necessary to make debuginstall happy.
if [ ! -f ~/.hgrc ]; then
  cat >> ~/.hgrc << EOF
[ui]
username = Dummy User <nobody@example.com>
EOF
fi

echo "Verifying Mercurial installation looks happy"
${INSTALL_DIR}/bin/hg debuginstall

. /etc/apache2/envvars

echo "Starting Apache HTTP Server on port 80"
echo "We hope you remembered to publish this port when running the container!"
echo "If this is an interactive container, simply CTRL^C to stop."

exec "$@"
