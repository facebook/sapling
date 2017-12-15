source "$RUNTESTDIR/helpers-testrepo.sh"

# some version of helpers-testrepo.sh does not do this, but we want it to
# remove the obsstore warning. so let's check HGRCPATH and do it again.
if grep -q createmarkers "$HGRCPATH"; then
    :
else
cat >> "$HGRCPATH" << EOF
[experimental]
evolution = createmarkers
EOF
fi

# go to repo root
cd "$TESTDIR"/..

# enable lz4revlog if it's required
if grep -q 'lz4revlog' .hg/requires; then
    cat >> "$HGRCPATH" <<EOF
[extensions]
lz4revlog=
EOF
fi

# sanity check whether hg actually works or not
if ! testrepohg log -r tip -T '{author}' >/dev/null 2>"$TESTTMP/hg-err-check";
then
    echo 'hg does not work'
    exit 1
fi

# hg might work but print "failed to ..." - treat that as an error
if [ -s "$TESTTMP/hg-err-check" ]; then
    echo 'hg outputs to stderr: ' `cat "$TESTTMP/hg-err-check"`
    exit 1
fi
