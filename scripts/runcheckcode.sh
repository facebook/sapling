cd tests

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

$MERCURIALRUNTEST -j8 -l test-check-code-hg.t --extra-config-opt=extensions.lz4revlog=
