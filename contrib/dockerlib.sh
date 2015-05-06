#!/bin/sh -eu

# This function exists to set up the DOCKER variable and verify that
# it's the binary we expect. It also verifies that the docker service
# is running on the system and we can talk to it.
function checkdocker() {
  if which docker.io >> /dev/null 2>&1 ; then
    DOCKER=docker.io
  elif which docker >> /dev/null 2>&1 ; then
    DOCKER=docker
  else
    echo "Error: docker must be installed"
    exit 1
  fi

  $DOCKER -h 2> /dev/null | grep -q Jansens && { echo "Error: $DOCKER is the Docking System Tray - install docker.io instead"; exit 1; }
  $DOCKER version | grep -q "^Client version:" || { echo "Error: unexpected output from \"$DOCKER version\""; exit 1; }
  $DOCKER version | grep -q "^Server version:" || { echo "Error: could not get docker server version - check it is running and your permissions"; exit 1; }
}
