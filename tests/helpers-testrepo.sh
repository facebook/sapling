# The test-repo is a live hg repository which may have evolution
# markers created, e.g. when a ~/.hgrc enabled evolution.
#
# Tests are run using a custom HGRCPATH, which do not
# enable evolution markers by default.
#
# If test-repo includes evolution markers, and we do not
# enable evolution markers, hg will occasionally complain
# when it notices them, which disrupts tests resulting in
# sporadic failures.
#
# Since we aren't performing any write operations on the
# test-repo, there's no harm in telling hg that we support
# evolution markers, which is what the following lines
# for the hgrc file do:
cat >> $HGRCPATH << EOF
[experimental]
evolution=createmarkers
EOF
