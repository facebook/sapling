#!/bin/sh
# This scripts is used to setup temp directory in memory
# for running Mercurial tests in vritual machine managed
# by Vagrant (see Vagrantfile for details).

cd /hgshared
make local
cd tests
mkdir /tmp/ram
sudo mount -t tmpfs -o size=100M tmpfs /tmp/ram
export TMPDIR=/tmp/ram
./run-tests.py -l --time

