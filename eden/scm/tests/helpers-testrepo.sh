# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# In most cases, the mercurial repository can be read by the bundled hg, but
# that isn't always true because third-party extensions may change the store
# format, for example. In which case, the system hg installation is used.
#
# We want to use the hg version being tested when interacting with the test
# repository, and the system hg when interacting with the mercurial source code
# repository.
#
# The mercurial source repository was typically orignally cloned with the
# system mercurial installation, and may require extensions or settings from
# the system installation.

# Revert the environment so that running "hg" runs the system hg
# rather than the test hg installation.
syshgenv () {
    # shellcheck disable=SC1090
    . "$HGTEST_RESTOREENV"
    HGPLAIN=1
    export HGPLAIN
}

# The test-repo is a live hg repository which may have evolution markers
# created, e.g. when a ~/.hgrc enabled evolution.
#
# Tests may be run using a custom HGRCPATH, which do not enable evolution
# markers by default.
#
# If test-repo includes evolution markers, and we do not enable evolution
# markers, hg will occasionally complain when it notices them, which disrupts
# tests resulting in sporadic failures.
#
# Since we aren't performing any write operations on the test-repo, there's
# no harm in telling hg that we support evolution markers, which is what the
# following lines for the hgrc file do:
cat >> "$HGRCPATH" << EOF
[experimental]
evolution = createmarkers
EOF


SYSTEM_HG_VER=$(syshgenv; hg --version -q 2>/dev/null)
case "$SYSTEM_HG_VER" in
    Sapling*)
        # Use the system hg environment if it has a has a chance
        # of reading a sapling repo
        testrepohgenv () {
            syshgenv
        }
        ;;
    *)
        testrepohgenv () {
            # no suitable system hg, stick current.
            :
        }
        ;;
esac

testrepohg () {
    (
        testrepohgenv
        # Silent potential stderr like "remove: served by Mononoke".
        exec hg "$@" 2>/dev/null
    )
}
