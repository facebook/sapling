  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > EOF

Test wrapped blame to be able to handle the usual command line attributes
  $ hg init repo
  $ cd repo
  $ echo "line one" > a
  $ echo "line two" >> a
  $ hg ci -Am "Differential Revision: https://phabricator.fb.com/D111111"
  adding a
  $ echo "line three" >> a
  $ hg ci -Am "Differential Revision: https://phabricator.fb.com/D222222"
  $ hg blame a
  37b9ff139054:line one
  37b9ff139054:line two
  05d474df3f59:line three
  $ hg blame --user a
  37b9ff139054:         test:line one
  37b9ff139054:         test:line two
  05d474df3f59:         test:line three
  $ hg blame --date a
  37b9ff139054:Thu, 01 Jan 1970 00:00:00 +0000:line one
  37b9ff139054:Thu, 01 Jan 1970 00:00:00 +0000:line two
  05d474df3f59:Thu, 01 Jan 1970 00:00:00 +0000:line three
  $ hg blame --number a
  0        :line one
  0        :line two
  1        :line three
  $ hg blame --file --line-number a
  37b9ff139054:a:    1:line one
  37b9ff139054:a:    2:line two
  05d474df3f59:a:    3:line three
  $ hg blame --user --date --line-number a
  37b9ff139054:         test:Thu, 01 Jan 1970 00:00:00 +0000:    1:line one
  37b9ff139054:         test:Thu, 01 Jan 1970 00:00:00 +0000:    2:line two
  05d474df3f59:         test:Thu, 01 Jan 1970 00:00:00 +0000:    3:line three
  $ hg blame -p a 
  37b9ff139054:D111111 :line one
  37b9ff139054:D111111 :line two
  05d474df3f59:D222222 :line three
