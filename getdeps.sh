#!/usr/bin/env bash

function update() {
    repo=$(basename "$1" .git)
    echo "updating $repo..."
    if [ -d "$repo" ]; then
        (cd "$repo" && git pull)
    else
        git clone "$1"
        [ -z "$2" ] || (cd "$repo" && git checkout "$2")
    fi
}

function build() {
    (
        echo "building $1..."
        cd "$1" || exit 1
        shift
        if [ ! -e ./configure ]; then
            autoreconf --install
        fi
        ./configure
        make -j8
    )
}

function build_cmake() {
    (
        echo "building $1..."
        mkdir -p "$1/build"
        cd "$1/build" || exit 1
        shift
        echo cmake .. "$@"
        cmake .. "$@"
        make
    )
}

function get_packages() {
    echo "installing packages"
    sudo apt-get install -yq autoconf automake libdouble-conversion-dev \
        libssl-dev make zip git libtool g++ libboost-all-dev \
        libevent-dev flex bison libgoogle-glog-dev libkrb5-dev \
        libsnappy-dev libsasl2-dev libnuma-dev libcurl4-gnutls-dev \
        libpcap-dev libdb5.3-dev cmake libfuse-dev libgit2-dev mercurial
}

if [ "$1" = 'pkg' ]; then
    get_packages
fi

echo "creating external..."
mkdir -p external
(
    cd external || exit 1
    EXTERNAL_DIR=$(pwd)
    update https://github.com/facebook/folly.git
    update https://github.com/facebook/wangle.git
    update https://github.com/facebook/fbthrift.git
    update https://github.com/no1msd/mstch.git
    update https://github.com/facebook/zstd.git
    update https://github.com/facebook/rocksdb.git
    update https://github.com/google/googletest.git
    build mstch
    build zstd
    build rocksdb
    build_cmake googletest
    build folly/folly
    build_cmake wangle/wangle \
        "-DFOLLY_INCLUDE_DIR=${EXTERNAL_DIR}/folly" \
        "-DFOLLY_LIBRARY=${EXTERNAL_DIR}/folly/folly/.libs/libfolly.a" \
        "-DBUILD_TESTS=OFF"
    export CPPFLAGS=" -I${EXTERNAL_DIR}/folly -I${EXTERNAL_DIR}/wangle -I${EXTERNAL_DIR}/mstch/include -I${EXTERNAL_DIR}/zstd/lib"
    export LDFLAGS="-L${EXTERNAL_DIR}/folly/folly/.libs/ -L${EXTERNAL_DIR}/wangle/wangle/build/lib -L${EXTERNAL_DIR}/mstch/build/src -L${EXTERNAL_DIR}/zstd/lib"
    build fbthrift/thrift
)
