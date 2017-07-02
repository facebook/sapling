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
    . "$HGTEST_RESTOREENV"
    HGPLAIN=1
    export HGPLAIN
}

# Most test-check-* sourcing this file run "hg files", which is not available
# in ancient versions of hg. So we double check if "syshg files" works and
# fallback to hg bundled in the repo.
syshg files -h >/dev/null 2>/dev/null
if [ $? -eq 0 ]; then
    alias testrepohg=syshg
    alias testrepohgenv=syshgenv
else
    alias testrepohg=hg
    alias testrepohgenv=:
fi
