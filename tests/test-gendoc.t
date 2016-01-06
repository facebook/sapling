#require docutils
#require gettext

Test document extraction

  $ HGENCODING=UTF-8
  $ export HGENCODING
  $ { echo C; ls "$TESTDIR/../i18n"/*.po | sort; } | while read PO; do
  >     LOCALE=`basename "$PO" .po`
  >     echo "% extracting documentation from $LOCALE"
  >     LANGUAGE=$LOCALE python "$TESTDIR/../doc/gendoc.py" >> gendoc-$LOCALE.txt 2> /dev/null || exit
  > 
  >     if [ $LOCALE != C ]; then
  >         if [ ! -f $TESTDIR/test-gendoc-$LOCALE.t ]; then
  >             echo missing test-gendoc-$LOCALE.t
  >         fi
  >         cmp -s gendoc-C.txt gendoc-$LOCALE.txt && echo "** NOTHING TRANSLATED ($LOCALE) **"
  >     fi
  > done; true
  % extracting documentation from C
  % extracting documentation from da
  % extracting documentation from de
  % extracting documentation from el
  % extracting documentation from fr
  % extracting documentation from it
  % extracting documentation from ja
  % extracting documentation from pt_BR
  % extracting documentation from ro
  % extracting documentation from ru
  % extracting documentation from sv
  % extracting documentation from zh_CN
  % extracting documentation from zh_TW
