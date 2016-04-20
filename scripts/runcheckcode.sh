#!/bin/bash

REPOROOT=$(dirname `readlink -f "$0"`)/../

# Look in $PATH
if hash run-tests.py 2>/dev/null; then
    MERCURIALRUNTEST=run-tests.py
fi

# Look directly the env variable
if [[ -z $MERCURIALRUNTEST ]]; then
    echo 'Please set env var MERCURIALRUNTEST to mercurial run-tests.py' ;
    echo '(or add run-tests.py to your $PATH)' ;
    exit 1 ;
fi

# Check lz4revlog requirement
RUNTESTOPTS=
if grep -q lz4revlog $REPOROOT/.hg/requires; then
    RUNTESTOPTS+='--extra-config-opt=extensions.lz4revlog='
fi

# Run test-check-code-hg.t
cd $REPOROOT/tests
$MERCURIALRUNTEST -j8 -l $RUNTESTOPTS test-check-code-hg.t
