Dirstate
========

Internals of the directory state cache.

Introduction
------------

Mercurial tracks various information about the working directory (the **dirstate**):

* what revision(s) are currently checked out

* what files have been copied or renamed

* what files are controlled by Mercurial

For each file that Mercurial controls, we record the following information:

* its size

* its mode

* its modification time

* its "state"

The states that are tracked are:

* n - normal

* a - added

* r - removed

* m - 3-way merged

With this information, we can quickly determine what files in the working directory have changed.

Here's a real example of a dirstate of a clone of the Mercurial repository itself:

::

   > hg up -q -C null

   > hg up -C -r 2000
   286 files updated, 0 files merged, 0 files removed, 0 files unresolved

   > hg debugstate
   n 666        204 2008-08-21 11:29:20 .hgignore
   n 666        502 2008-08-21 11:29:20 .hgtags
   n 666       1508 2008-08-21 11:29:20 CONTRIBUTORS
   n 666      17992 2008-08-21 11:29:20 COPYING
   n 666        486 2008-08-21 11:29:20 MANIFEST.in
   n 666        555 2008-08-21 11:29:20 Makefile
   n 666       2733 2008-08-21 11:29:20 README
   n 666       1669 2008-08-21 11:29:20 comparison.txt
   n 666       3229 2008-08-21 11:29:20 contrib/bash_completion
   n 666       1305 2008-08-21 11:29:20 contrib/buildrpm
   n 666       8721 2008-08-21 11:29:20 contrib/convert-repo
   n 666       2058 2008-08-21 11:29:20 contrib/favicon.ico
   n 666         69 2008-08-21 11:29:20 contrib/git-viz/git-cat-file
   n 666         69 2008-08-21 11:29:20 contrib/git-viz/git-diff-tree
   n 666         69 2008-08-21 11:29:20 contrib/git-viz/git-rev-list
   n 666         69 2008-08-21 11:29:20 contrib/git-viz/git-rev-tree
   n 666        457 2008-08-21 11:29:20 contrib/git-viz/hg-viz
   n 666       3534 2008-08-21 11:29:20 contrib/hg-menu.vim
   n 666       1681 2008-08-21 11:29:20 contrib/hg-ssh
   n 666       3048 2008-08-21 11:29:20 contrib/hgdiff
   n 666      97510 2008-08-21 11:29:20 contrib/hgk
   n 666      11664 2008-08-21 11:29:20 contrib/hgk.py
   n 666       1987 2008-08-21 11:29:20 contrib/macosx/Readme.html
   n 666        668 2008-08-21 11:29:20 contrib/macosx/Welcome.html
   n 666        266 2008-08-21 11:29:20 contrib/macosx/macosx-build.txt
   n 666      37273 2008-08-21 11:29:20 contrib/mercurial.el
   n 666       1004 2008-08-21 11:29:20 contrib/mercurial.spec
   n 666       1227 2008-08-21 11:29:20 contrib/tcsh_completion
   n 666       1902 2008-08-21 11:29:20 contrib/tcsh_completion_build.sh
   n 666       4288 2008-08-21 11:29:20 contrib/win32/ReadMe.html
   n 666       1264 2008-08-21 11:29:20 contrib/win32/mercurial.ini
   n 666       2390 2008-08-21 11:29:20 contrib/win32/mercurial.iss
   n 666       1920 2008-08-21 11:29:20 contrib/win32/postinstall.txt
   n 666       1194 2008-08-21 11:29:20 contrib/win32/win32-build.txt
   n 666      16852 2008-08-21 11:29:20 contrib/zsh_completion
   n 666        457 2008-08-21 11:29:20 doc/Makefile
   n 666        529 2008-08-21 11:29:20 doc/README
   n 666       2727 2008-08-21 11:29:20 doc/gendoc.py
   n 666       6928 2008-08-21 11:29:20 doc/hg.1.txt
   n 666        827 2008-08-21 11:29:20 doc/hgmerge.1.txt
   n 666      11992 2008-08-21 11:29:20 doc/hgrc.5.txt
   n 666        350 2008-08-21 11:29:20 doc/ja/Makefile
   n 666      13642 2008-08-21 11:29:20 doc/ja/docbook.ja.conf
   n 666        734 2008-08-21 11:29:20 doc/ja/docbook.ja.xsl
   n 666      40128 2008-08-21 11:29:20 doc/ja/hg.1.ja.txt
   n 666       1090 2008-08-21 11:29:20 doc/ja/hgmerge.1.ja.txt
   n 666       8097 2008-08-21 11:29:20 doc/ja/hgrc.5.ja.txt
   n 666        307 2008-08-21 11:29:20 hg
   n 666       1138 2008-08-21 11:29:20 hgeditor
   n 666         14 2008-08-21 11:29:20 hgext/__init__.py
   n 666       8552 2008-08-21 11:29:20 hgext/gpg.py
   n 666      10097 2008-08-21 11:29:20 hgext/hbisect.py
   n 666      45715 2008-08-21 11:29:20 hgext/mq.py
   n 666      10124 2008-08-21 11:29:20 hgext/patchbomb.py
   n 666        592 2008-08-21 11:29:20 hgext/win32text.py
   n 666       4919 2008-08-21 11:29:20 hgmerge
   n 666        283 2008-08-21 11:29:20 hgweb.cgi
   n 666        892 2008-08-21 11:29:20 hgwebdir.cgi
   n 666          0 2008-08-21 11:29:20 mercurial/__init__.py
   n 666       5751 2008-08-21 11:29:20 mercurial/appendfile.py
   n 666       7767 2008-08-21 11:29:20 mercurial/bdiff.c
   n 666       7664 2008-08-21 11:29:20 mercurial/bundlerepo.py
   n 666      16601 2008-08-21 11:29:20 mercurial/byterange.py
   n 666       1116 2008-08-21 11:29:20 mercurial/changegroup.py
   n 666       2075 2008-08-21 11:29:20 mercurial/changelog.py
   n 666     119453 2008-08-21 11:29:21 mercurial/commands.py
   n 666       4592 2008-08-21 11:29:21 mercurial/demandload.py
   n 666      14293 2008-08-21 11:29:21 mercurial/dirstate.py
   n 666        847 2008-08-21 11:29:21 mercurial/fancyopts.py
   n 666       3378 2008-08-21 11:29:21 mercurial/filelog.py
   n 666       1368 2008-08-21 11:29:21 mercurial/hg.py
   n 666      38438 2008-08-21 11:29:21 mercurial/hgweb.py
   n 666        770 2008-08-21 11:29:21 mercurial/httprangereader.py
   n 666       4826 2008-08-21 11:29:21 mercurial/httprepo.py
   n 666        444 2008-08-21 11:29:21 mercurial/i18n.py
   n 666      71466 2008-08-21 11:29:21 mercurial/localrepo.py
   n 666       3210 2008-08-21 11:29:21 mercurial/lock.py
   n 666       6626 2008-08-21 11:29:21 mercurial/manifest.py
   n 666       6101 2008-08-21 11:29:21 mercurial/mdiff.py
   n 666       7620 2008-08-21 11:29:21 mercurial/mpatch.c
   n 666        422 2008-08-21 11:29:21 mercurial/node.py
   n 666       2860 2008-08-21 11:29:21 mercurial/packagescan.py
   n 666        555 2008-08-21 11:29:21 mercurial/remoterepo.py
   n 666        274 2008-08-21 11:29:21 mercurial/repo.py
   n 666      30641 2008-08-21 11:29:21 mercurial/revlog.py
   n 666       3991 2008-08-21 11:29:21 mercurial/sshrepo.py
   n 666       1534 2008-08-21 11:29:21 mercurial/statichttprepo.py
   n 666       9759 2008-08-21 11:29:21 mercurial/templater.py
   n 666       2513 2008-08-21 11:29:21 mercurial/transaction.py
   n 666       8662 2008-08-21 11:29:21 mercurial/ui.py
   n 666      25290 2008-08-21 11:29:21 mercurial/util.py
   n 666       2148 2008-08-21 11:29:21 mercurial/version.py
   n 666       6251 2008-08-21 11:29:21 notes.txt
   n 666        507 2008-08-21 11:29:21 rewrite-log
   n 666       3941 2008-08-21 11:29:21 setup.py
   n 666        997 2008-08-21 11:29:21 templates/changelog-gitweb.tmpl
   n 666        155 2008-08-21 11:29:21 templates/changelog-rss.tmpl
   n   0         -1 unset               templates/changelog.tmpl
   n   0         -1 unset               templates/changelogentry-gitweb.tmpl
   n   0         -1 unset               templates/changelogentry-rss.tmpl
   n   0         -1 unset               templates/changelogentry.tmpl
   n   0         -1 unset               templates/changeset-gitweb.tmpl
   n   0         -1 unset               templates/changeset-raw.tmpl
   n   0         -1 unset               templates/changeset.tmpl
   n   0         -1 unset               templates/error-gitweb.tmpl
   n   0         -1 unset               templates/error.tmpl
   n   0         -1 unset               templates/fileannotate-gitweb.tmpl
   n   0         -1 unset               templates/fileannotate-raw.tmpl
   n   0         -1 unset               templates/fileannotate.tmpl
   n   0         -1 unset               templates/filediff-raw.tmpl
   n   0         -1 unset               templates/filediff.tmpl
   n   0         -1 unset               templates/filelog-gitweb.tmpl
   n   0         -1 unset               templates/filelog-rss.tmpl
   n   0         -1 unset               templates/filelog.tmpl
   n   0         -1 unset               templates/filelogentry-rss.tmpl
   n   0         -1 unset               templates/filelogentry.tmpl
   n   0         -1 unset               templates/filerevision-gitweb.tmpl
   n   0         -1 unset               templates/filerevision-raw.tmpl
   n   0         -1 unset               templates/filerevision.tmpl
   n   0         -1 unset               templates/footer-gitweb.tmpl
   n   0         -1 unset               templates/footer.tmpl
   n   0         -1 unset               templates/header-gitweb.tmpl
   n   0         -1 unset               templates/header-raw.tmpl
   n   0         -1 unset               templates/header-rss.tmpl
   n   0         -1 unset               templates/header.tmpl
   n   0         -1 unset               templates/index.tmpl
   n   0         -1 unset               templates/manifest-gitweb.tmpl
   n   0         -1 unset               templates/manifest.tmpl
   n   0         -1 unset               templates/map
   n   0         -1 unset               templates/map-cmdline.changelog
   n   0         -1 unset               templates/map-cmdline.compact
   n   0         -1 unset               templates/map-cmdline.default
   n   0         -1 unset               templates/map-gitweb
   n   0         -1 unset               templates/map-raw
   n   0         -1 unset               templates/map-rss
   n   0         -1 unset               templates/notfound.tmpl
   n   0         -1 unset               templates/search-gitweb.tmpl
   n   0         -1 unset               templates/search.tmpl
   n   0         -1 unset               templates/shortlog-gitweb.tmpl
   n   0         -1 unset               templates/static/hgicon.png
   n   0         -1 unset               templates/static/style-gitweb.css
   n   0         -1 unset               templates/static/style.css
   n   0         -1 unset               templates/summary-gitweb.tmpl
   n   0         -1 unset               templates/tagentry-rss.tmpl
   n   0         -1 unset               templates/tags-gitweb.tmpl
   n   0         -1 unset               templates/tags-rss.tmpl
   n   0         -1 unset               templates/tags.tmpl
   n   0         -1 unset               templates/template-vars.txt
   n   0         -1 unset               tests/README
   n   0         -1 unset               tests/fish-merge
   n   0         -1 unset               tests/md5sum.py
   n   0         -1 unset               tests/run-tests
   n   0         -1 unset               tests/test-addremove
   n   0         -1 unset               tests/test-addremove.out
   n   0         -1 unset               tests/test-archive
   n   0         -1 unset               tests/test-archive.out
   ...

For files having state "n" in the dirstate, Mercurial compares the file modification time and the size in the dirstate with the modification time and the size of the file in the working directory. If both the modification time *and* the size are the same, Mercurial will assume it has not changed and will thus not include it in the next commit.

Having size "-1" and date "unset" means that Mercurial assumes nothing about the contents of that file and will have to look into the file to determine whether it has changed or not. See also an explanation given by Matt Mackall in http://selenic.com/pipermail/mercurial/2008-August/020984.html

File format
-----------

::

   .hg/dirstate:
   <p1 binhash><p2 binhash>
   <list of dirstate entries>

a dirstate entry is composed of:

::

   8bit: status
   32bit: mode
   32bit: size
   32bit: mtime
   32bit: length
   variable length entry (length given by the previous length field) with:
   "<filename>" followed if it's a copy by: "\0<source if copy>"

status can be either:

* 'n': normal

* 'm': merged

* 'a': added

* 'r': removed

Details of the semantics
~~~~~~~~~~~~~~~~~~~~~~~~

mode stores the st.st_mod of the file as it was clean, but only the user x-bit is ever checked

size is usually the size of the file, as it was stored (after any potential filters). If size is -1 or -2, it has a different semantic. First -1, in conjunction with mtime can be used to force a lookup. Second, they are used when the dirstate is in a merge state (p1 != nullid): -2 will *always* return dirty; it is used to mark a file that was cleanly picked from p2 with a status of 'r', -2 means that the previous state was -2 (always dirty, picked from p2), -1 means the previous status was 'm' (merged), those allows revert to pick the right status back during a merge.

mtime is usually the mtime of the file when it was last clean. If the size is < 0, setting -1 as mtime will force a lookup (and allows us to correctly deal with changes done less than one second after we updated the dirstate).

Summary
-------

In summary, we have the additional "meta" status:

* 'nl' : normallookup (status == 'n', size == -1, mtime == -1 (or sometimes 0))

* 'np2': merged from other parent (status == 'n', size == -2)

* 'rm' : removed and previous state was 'm' (status == 'r', size == -1)

* 'rp2': removed and previous state was 'np2' (status == 'r', size == -2)

And we can notice that no bits from mode are used, except 0x40 (user x-bit). Assuming the bits from stat.st_mode are portable across platfroms and OSs, the upper bits are set in the following way (in binary)

::

   S_IFIFO  0001 /* FIFO.  */
   S_IFCHR  0010 /* Character device.  */
   S_IFLNK  1010 /* Symbolic link.  */
   S_IFBLK  0110 /* Block device.  */
   S_IFDIR  0100 /* Directory.  */
   S_IFREG  1000 /* Regular file.  */
   S_IFSOCK 1100 /* Socket.  */

Since hg should only add regular files or symlinks to the dirstate, it means we can signal the presence of the extended dirstate entry by setting either 0100, or 0001. Then we can use the remaining bits (30 free bits!) to encode whatever we want.

Proposed extensions
-------------------

* 'l' flag (is the entry a symlink)

* 'fallback-x': should the on-disk file be considered as having the x-bit set, useful if the FS doesn't support exec bit, the bit can still be changed with a git patch).

* 'fallback-l': should the on disk-file be considered a symlink (useful if the FS doesn't support symlinks, they can still be added to the repo, with hg import and a git patch for example)

* correctly mark 'np2', for merges we can use a bit to indicate if the file is clean from p1 or from p2.

* anything else?

