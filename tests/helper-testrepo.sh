source "$RUNTESTDIR/helpers-testrepo.sh"

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
if ! hg log -r tip -T '{author}' >/dev/null 2>"$TESTTMP/hg-err-check"; then
    echo 'skipped: missing working hg'
    exit 80
fi

# hg might work but print "failed to ..." - treat that as an error
if [ -s "$TESTTMP/hg-err-check" ]; then
    echo 'skipped: missing working hg'
    exit 80
fi
