#!/bin/sh
# This scripts is used to install dependencies for
# testing Mercurial. Mainly used by Vagrant (see
# Vagrantfile for details).

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y -q python-dev unzip
# run-tests.sh is added by Vagrantfile
chmod +x run-tests.sh
