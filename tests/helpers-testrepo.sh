# Invoke the system hg installation (rather than the local hg version being
# tested).
#
# We want to use the hg version being tested when interacting with the test
# repository, and the system hg when interacting with the mercurial source code
# repository.
#
# The mercurial source repository was typically orignally cloned with the
# system mercurial installation, and may require extensions or settings from
# the system installation.
syshg () {
    (
        syshgenv
        exec hg "$@"
    )
}

# Revert the environment so that running "hg" runs the system hg
# rather than the test hg installation.
syshgenv () {
    PATH="$ORIG_PATH"
    PYTHONPATH="$ORIG_PYTHONPATH"
    JYTHONPATH="$ORIG_JYTHONPATH"
    unset HGRCPATH
    HGPLAIN=1
    export HGPLAIN
}
