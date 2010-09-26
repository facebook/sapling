Test document extraction

  $ "$TESTDIR/hghave" docutils || exit 80
  $ HGENCODING=UTF-8
  $ export HGENCODING
  $ for PO in C $TESTDIR/../i18n/*.po; do
  >     LOCALE=`basename $PO .po`
  >     echo
  >     echo "% extracting documentation from $LOCALE"
  >     echo ".. -*- coding: utf-8 -*-" > gendoc-$LOCALE.txt
  >     echo "" >> gendoc-$LOCALE.txt
  >     LC_ALL=$LOCALE python $TESTDIR/../doc/gendoc.py >> gendoc-$LOCALE.txt 2> /dev/null || exit
  > 
  >     # We call runrst without adding "--halt warning" to make it report
  >     # all errors instead of stopping on the first one.
  >     echo "checking for parse errors"
  >     python $TESTDIR/../doc/runrst html gendoc-$LOCALE.txt /dev/null
  > done
  
  % extracting documentation from C
  checking for parse errors
  
  % extracting documentation from da
  checking for parse errors
  
  % extracting documentation from de
  checking for parse errors
  
  % extracting documentation from el
  checking for parse errors
  
  % extracting documentation from fr
  checking for parse errors
  
  % extracting documentation from it
  checking for parse errors
  
  % extracting documentation from ja
  checking for parse errors
  
  % extracting documentation from pt_BR
  checking for parse errors
  
  % extracting documentation from ro
  checking for parse errors
  
  % extracting documentation from sv
  checking for parse errors
  
  % extracting documentation from zh_CN
  checking for parse errors
  
  % extracting documentation from zh_TW
  checking for parse errors
