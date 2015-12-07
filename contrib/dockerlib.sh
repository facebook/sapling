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
  $DOCKER version | grep -Eq "^Client( version)?:" || { echo "Error: unexpected output from \"$DOCKER version\""; exit 1; }
  $DOCKER version | grep -Eq "^Server( version)?:" || { echo "Error: could not get docker server version - check it is running and your permissions"; exit 1; }
}

# Construct a container and leave its name in $CONTAINER for future use.
function initcontainer() {
  [ "$1" ] || { echo "Error: platform name must be specified"; exit 1; }

  DFILE="$ROOTDIR/contrib/docker/$1"
  [ -f "$DFILE" ] || { echo "Error: docker file $DFILE not found"; exit 1; }

  CONTAINER="hg-dockerrpm-$1"
  DBUILDUSER=build
  (
    cat $DFILE
    if [ $(uname) = "Darwin" ] ; then
        # The builder is using boot2docker on OS X, so we're going to
        # *guess* the uid of the user inside the VM that is actually
        # running docker. This is *very likely* to fail at some point.
        echo RUN useradd $DBUILDUSER -u 1000
    else
        echo RUN groupadd $DBUILDUSER -g `id -g` -o
        echo RUN useradd $DBUILDUSER -u `id -u` -g $DBUILDUSER -o
    fi
  ) | $DOCKER build --tag $CONTAINER -
}
