#require test-repo

Enable obsolescence to avoid the warning issue when obsmarker are found

  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

Go back in the hg repo

  $ cd $TESTDIR/..

  $ for node in `hg log --rev 'draft() and ::.' --template '{node|short}\n'`; do
  >    hg export $node | contrib/check-commit > ${TESTTMP}/check-commit.out
  >    if [ $? -ne 0 ]; then
  >        echo "Revision $node does not comply to commit message rules"
  >        echo '------------------------------------------------------'
  >        cat ${TESTTMP}/check-commit.out
  >        echo
  >   fi
  > done


