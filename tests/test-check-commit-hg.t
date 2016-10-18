#require test-repo

Skip the test if check-commit is unavailable. It happens if we only have
run-tests.py but do not have the core hg repo.

  $ . $TESTDIR/require-core-hg.sh contrib/check-commit

This file is backported from mercurial/tests/test-check-commit.t.
It differs slightly to fix paths and prevent bypassing.

Enable obsolescence to avoid the warning issue when obsmarker are found

  $ . "$RUNTESTDIR/helpers-testrepo.sh"

Go back in the current repo (fb-hgext)

  $ cd $TESTDIR/..
  $ unset BYPASS

  $ for node in `hg log --rev 'not public() and ::.' --template '{node|short}\n'`; do
  >    hg export $node | $RUNTESTDIR/../contrib/check-commit > ${TESTTMP}/check-commit.out
  >    if [ $? -ne 0 ]; then
  >        echo "Revision $node does not comply with rules"
  >        echo '------------------------------------------------------'
  >        cat ${TESTTMP}/check-commit.out
  >        echo
  >    fi
  > done

