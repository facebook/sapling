#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg files -I . \
  > -X contrib/python-zstandard \
  > -X hgext/fsmonitor/pywatchman \
  > -X lib/cdatapack \
  > -X lib/third-party \
  > -X mercurial/thirdparty \
  > -X fb-hgext \
  > -X fb/facebook-hg-rpms \
  > -X fb/packaging \
  > | sed 's-\\-/-g' | "$check_code" --warnings --per-file=0 - || false
  fb/tests/sqldirstate_benchmark.py:95:
   >             hg.next()
   don't use .next(), use next(...)
  fb/tests/test-hg-rsh.t:2:
   >   $ HGRSH=$TESTDIR/../staticfiles/bin/hg-rsh
   don't use explicit paths for tools
  fb/tests/test-hg-rsh.t:35:
   >   > %include /bin/../etc/mercurial/repo-specific/common.rc
   don't use explicit paths for tools
  hgext/churn.py:98:
   >     return rate
   use single blank line
  hgext/convert/convcmd.py:230:
   >         return m
   use single blank line
  hgext/convert/cvsps.py:526:
   >     return log
   use single blank line
  hgext/convert/cvsps.py:850:
   >     return changesets
   use single blank line
  hgext/dirsync.py:229:
   >                 dirstate.add(mirrorpath)
   use single blank line
  hgext/drop.py:51:
   > testedwith = 'ships-with-fb-hgext'
   use single blank line
  hgext/drop.py:59:
   >         return None
   use single blank line
  hgext/drop.py:70:
   >     displayer.show(repo[revid])
   use single blank line
  hgext/drop.py:75:
   >     rebasemod = _checkextension('rebase', ui)
   use single blank line
  hgext/eol.py:311:
   >         pass
   use single blank line
  Skipping hgext/extlib/cfastmanifest.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/bsearch.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/bsearch.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/bsearch_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/checksum.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/checksum.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/checksum_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/internal_result.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/node.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/node.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/node_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/path_buffer.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/result.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tests.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tests.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_arena.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_arena.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_convert.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_convert_rt.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_convert_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_copy.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_copy_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_diff.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_diff_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_disk.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_disk_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_dump.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_iterate_rt.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_iterator.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_iterator.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_iterator_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_path.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_path.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cfastmanifest/tree_test.c it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/datapackstore.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/datapackstore.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/datastore.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/deltachain.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/deltachain.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/key.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/match.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/py-cdatapack.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/py-cstore.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/py-datapackstore.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/py-structs.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/py-treemanifest.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/pythondatastore.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/pythondatastore.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/pythonkeyiterator.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/pythonutil.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/pythonutil.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/store.h it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/uniondatapackstore.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/cstore/uniondatapackstore.h it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest.h it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_entry.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_entry.h it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_fetcher.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_fetcher.h it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_ptr.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/manifest_ptr.h it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/treemanifest.cpp it has no-che?k-code (glob)
  Skipping hgext/extlib/ctreemanifest/treemanifest.h it has no-che?k-code (glob)
  hgext/fastannotate/__init__.py:6:
   > # GNU General Public License version 2 or any later version.
   use single blank line
  hgext/fastannotate/commands.py:43:
   >         reldir = os.path.relpath(os.getcwd(), reporoot)
   use pycompat.getcwd instead (py3)
  hgext/fastlog.py:286:
   >     return orig(repo, pats, opts)
   use single blank line
  hgext/fastlog.py:300:
   >         return self._changelog.rev(node)
   use single blank line
  hgext/fastlog.py:369:
   >             queue.put((self.id, True, None))
   use single blank line
  hgext/fastmanifest/__init__.py:10:
   > patterns.
   use single blank line
  hgext/fastmanifest/cachemanager.py:320:
   >     repos_to_update = set()
   use single blank line
  hgext/fbhistedit.py:326:
   >         return orig(ui, repo, **opts)
   use single blank line
  hgext/fbshow.py:19:
   >   longer
   use single blank line
  hgext/fbshow.py:27:
   >   +more
   use single blank line
  hgext/fbshow.py:39:
   >   longer
   use single blank line
  hgext/fbsparse.py:1205:
   >     cwd = util.normpath(os.getcwd())
   use pycompat.getcwd instead (py3)
  hgext/hggit/__init__.py:81:
   >     pass
   use single blank line
  hgext/hggit/__init__.py:168:
   > extensions.wrapfunction(hgutil, 'url', _url)
   use single blank line
  hgext/hggit/_ssh.py:9:
   >     """Parent class for ui-linked Vendor classes."""
   use single blank line
  hgext/hggit/compat.py:62:
   >     return refs, set(server_capabilities)
   use single blank line
  hgext/hggit/git_handler.py:34:
   > from overlay import overlayrepo
   use single blank line
  hgext/hggit/hg2git.py:25:
   >     return sub, substate
   use single blank line
  hgext/hggit/hg2git.py:68:
   >                 % path)
   use single blank line
  Skipping hgext/hgsql.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/__init__.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/compathacks.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/editor.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/hooks/updatemeta.py it has no-che?k-code (glob)
  hgext/hgsubversion/layouts/__init__.py:1:
   > """Code for dealing with subversion layouts
   don't capitalize docstring title
  hgext/hgsubversion/layouts/__init__.py:29:
   > }
   use single blank line
  Skipping hgext/hgsubversion/layouts/base.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/layouts/custom.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/layouts/standard.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/maps.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/pushmod.py it has no-che?k-code (glob)
  hgext/hgsubversion/replay.py:11:
   > import util
   use single blank line
  Skipping hgext/hgsubversion/stupid.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svncommands.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnexternals.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnmeta.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnrepo.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnwrap/__init__.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnwrap/common.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnwrap/subvertpy_wrapper.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/svnwrap/svn_swig_wrapper.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/util.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/verify.py it has no-che?k-code (glob)
  Skipping hgext/hgsubversion/wrappers.py it has no-che?k-code (glob)
  hgext/histedit.py:412:
   >         return self.repo.vfs.exists('histedit-state')
   use single blank line
  hgext/histedit.py:710:
   >                 _("cannot fold into public change %s") % node.short(c.node()))
   use single blank line
  hgext/morestatus.py:35:
   >     return '\n'.join(commentedlines) + '\n'
   use single blank line
  hgext/morestatus.py:120:
   > )
   use single blank line
  hgext/morestatus.py:49:
   >                 os.getcwd()) for path in unresolvedlist])
   use pycompat.getcwd instead (py3)
  hgext/mq.py:141:
   > checklocalchanges = stripext.checklocalchanges
   use single blank line
  hgext/mq.py:2299:
   >               summary=opts.get('summary'))
   use single blank line
  hgext/mq.py:3115:
   >     return 0
   use single blank line
  hgext/phabstatus.py:78:
   >             repodir=os.getcwd(), ca_bundle=ca_certs, repo=repo)
   use pycompat.getcwd instead (py3)
  hgext/pushrebase.py:175:
   >             repo = orig(ui, bundlepath, create=create, **kwargs)
   use single blank line
  hgext/pushrebase.py:668:
   >             return None
   use single blank line
  hgext/pushrebase.py:712:
   >         files = rev.files()
   use single blank line
  hgext/record.py:30:
   > testedwith = 'ships-with-hg-core'
   use single blank line
  hgext/record.py:126:
   >     return origfn(ui, repo, patch, *args, **opts)
   use single blank line
  hgext/remotefilelog/__init__.py:779:
   >         ui.warn(_("warning: no valid repos in repofile\n"))
   use single blank line
  hgext/remotefilelog/remotefilelog.py:384:
   >                     allparents.add(p2)
   use single blank line
  hgext/remotefilelog/repack.py:88:
   >                    constants.TREEPACK_CATEGORY, options=options)
   use single blank line
  hgext/remotefilelog/shallowutil.py:317:
   >         f.close()
   use single blank line
  hgext/remotefilelog/shallowutil.py:325:
   >     util.unlink(filepath)
   use single blank line
  hgext/remotefilelog/shallowutil.py:337:
   >     os.rename(source, destination)
   use single blank line
  hgext/remotenames.py:612:
   >         return self._node2branch
   use single blank line
  hgext/smartlog.py:533:
   >     return subset & revs
   use single blank line
  hgext/strip.py:235:
   >             update = False
   use single blank line
  hgext/tweakdefaults.py:275:
   >     if pipei_bufsize != 4096 and os.name == 'nt':
   use pycompat.osname instead (py3)
  hgext/undo.py:71:
   >     if 'CHGINTERNALMARK' in os.environ:
   use encoding.environ instead (py3)
  hgext/undo.py:89:
   >     if '_undologactive' in os.environ:
   use encoding.environ instead (py3)
  hgext/undo.py:97:
   >             os.environ['_undologactive'] = "active"
   use encoding.environ instead (py3)
  hgext/undo.py:127:
   >                 del os.environ['_undologactive']
   use encoding.environ instead (py3)
  hgext/win32mbcs.py:112:
   >     return s
   use single blank line
  hgext/win32mbcs.py:130:
   >     return basewrapper(func, unicode, encode, decode, args, kwds)
   use single blank line
  hgext/zeroconf/Zeroconf.py:286:
   >         return DNSEntry.toString(self, "question", None)
   use single blank line
  hgext/zeroconf/Zeroconf.py:646:
   >         return result
   use single blank line
  hgext/zeroconf/Zeroconf.py:808:
   >         return ''.join(self.data)
   use single blank line
  hgext/zeroconf/Zeroconf.py:860:
   >             return []
   use single blank line
  hgext/zeroconf/Zeroconf.py:965:
   >             self.zeroconf.handleResponse(msg)
   use single blank line
  hgext/zeroconf/Zeroconf.py:986:
   >                     self.zeroconf.cache.remove(record)
   use single blank line
  hgext/zeroconf/Zeroconf.py:1068:
   >                 event(self.zeroconf)
   use single blank line
  hgext/zeroconf/Zeroconf.py:1297:
   >         return result
   use single blank line
  Skipping hgsubversion/setup.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping lib/clib/buffer.c it has no-che?k-code (glob)
  Skipping lib/clib/buffer.h it has no-che?k-code (glob)
  Skipping lib/clib/convert.h it has no-che?k-code (glob)
  Skipping lib/clib/null_test.c it has no-che?k-code (glob)
  Skipping lib/clib/portability/inet.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/portability.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/unistd.h it has no-che?k-code (glob)
  Skipping lib/clib/sha1.h it has no-che?k-code (glob)
  mercurial/bundle2.py:11:
   > that will be handed to and processed by the application layer.
   use single blank line
  mercurial/bundle2.py:686:
   >         yield _pack(_fpartheadersize, 0)
   use single blank line
  mercurial/bundle2.py:698:
   >         return salvaged
   use single blank line
  mercurial/bundle2.py:784:
   >         return params
   use single blank line
  mercurial/bundle2.py:848:
   >             yield self._readexact(size)
   use single blank line
  mercurial/bundle2.py:1122:
   >             yield self.data
   use single blank line
  mercurial/bundle2.py:2070:
   >         rpart.addparam('new', '%i' % new, mandatory=False)
   use single blank line
  mercurial/byterange.py:244:
   >         return urlreq.addinfourl(fo, headers, 'file:'+file)
   use single blank line
  mercurial/byterange.py:393:
   >                             self.endtransfer), conn[1])
   use single blank line
  mercurial/commands.py:1243:
   >         compopts['level'] = complevel
   use single blank line
  mercurial/commands.py:2666:
   >     ui.write(formatted)
   use single blank line
  mercurial/commands.py:3001:
   >     ret = 0
   use single blank line
  mercurial/commands.py:3157:
   >         del repo._subtoppath
   use single blank line
  mercurial/commands.py:3966:
   >                                           opts.get('rev'))
   use single blank line
  mercurial/dagutil.py:85:
   >         return list(ixs)
   use single blank line
  mercurial/dagutil.py:113:
   >         return hds
   use single blank line
  mercurial/dagutil.py:151:
   >         return [self._internalize(i) for i in ids]
   use single blank line
  mercurial/dagutil.py:245:
   >         return sorted
   use single blank line
  mercurial/dispatch.py:19:
   > import traceback
   use single blank line
  mercurial/exchange.py:105:
   >         return version, params
   use single blank line
  mercurial/exchange.py:415:
   >               }
   use single blank line
  mercurial/exchange.py:1399:
   >     pullop.remotebookmarks = bookmod.unhexlifybookmarks(books)
   use single blank line
  mercurial/hbisect.py:146:
   >     return state
   use single blank line
  mercurial/help.py:406:
   >         return rst
   use single blank line
  mercurial/help.py:581:
   >         return rst
   use single blank line
  mercurial/hgweb/common.py:31:
   > HTTP_SERVER_ERROR = 500
   use single blank line
  mercurial/hgweb/common.py:89:
   > permhooks = [checkauthz]
   use single blank line
  mercurial/hgweb/hgweb_mod.py:220:
   >         return tmpl
   use single blank line
  mercurial/hgweb/webcommands.py:1151:
   >     return []
   use single blank line
  mercurial/hgweb/webutil.py:124:
   >                 navbefore.append(("-%d" % abs(rev - pos), self.hex(rev)))
   use single blank line
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  mercurial/httpconnection.py:129:
   >                                           headers=headers)
   use single blank line
  mercurial/keepalive.py:363:
   >     # modification from socket.py
   use single blank line
  mercurial/keepalive.py:591:
   >     getresponse = wrapgetresponse(httplib.HTTPConnection)
   use single blank line
  mercurial/keepalive.py:596:
   > #########################################################################
   use single blank line
  mercurial/keepalive.py:705:
   >     DEBUG = dbbackup
   use single blank line
  mercurial/localrepo.py:132:
   >         object.__setattr__(obj, self.name, value)
   use single blank line
  mercurial/localrepo.py:874:
   >         # quo fine?
   use single blank line
  mercurial/lsprof.py:21:
   >     return Stats(p.getstats())
   use single blank line
  mercurial/lsprof.py:109:
   >     return '%s:%d(%s)' % (mname, code.co_firstlineno, code.co_name)
   use single blank line
  mercurial/manifest.py:697:
   >                 _("'\\n' and '\\r' disallowed in filenames: %r") % f)
   use single blank line
  mercurial/merge.py:1385:
   >         yield i, f
   use single blank line
  mercurial/minirst.py:414:
   >     return blocks
   use single blank line
  mercurial/obsolete.py:1044:
   >     return divergent
   use single blank line
  mercurial/patch.py:717:
   >             self.ui.note(s)
   use single blank line
  mercurial/phases.py:14:
   > This module implements most phase logic in mercurial.
   use single blank line
  mercurial/phases.py:88:
   >     passive = only pushes
   use single blank line
  mercurial/phases.py:649:
   >     return [c.node() for c in revset]
   use single blank line
  mercurial/pure/diffhelpers.py:51:
   >     return 0
   use single blank line
  mercurial/pure/parsers.py:15:
   > stringio = pycompat.stringio
   use single blank line
  mercurial/repair.py:225:
   >         repo._phasecache.invalidate()
   use single blank line
  mercurial/repoview.py:47:
   >     return pinned
   use single blank line
  mercurial/revlog.py:2377:
   >                     rawtext = self.revision(rev, raw=True)
   use single blank line
  mercurial/revset.py:2038:
   >     return subset & orphan
   use single blank line
  mercurial/simplemerge.py:259:
   >             # that's OK, we can just skip it.
   use single blank line
  Skipping mercurial/statprof.py it has no-che?k-code (glob)
  mercurial/subrepo.py:20:
   > import xml.dom.minidom
   use single blank line
  mercurial/subrepo.py:1455:
   >         return self._svncommand(['cat'], name)[0]
   use single blank line
  mercurial/subrepo.py:1971:
   >         return total
   use single blank line
  mercurial/subrepo.py:1991:
   >         return 0
   use single blank line
  mercurial/tags.py:450:
   >         return ([], {}, valid, None, True)
   use single blank line
  mercurial/tags.py:650:
   >         self.hitcount = 0
   use single blank line
  mercurial/transaction.py:545:
   >         undobackupfile.close()
   use single blank line
  Skipping tests/badserverext.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-custom-layout.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-obsstore-on.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-rebuildmeta.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-sqlite-revmap.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-stupid-pull.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-updatemeta.py it has no-che?k-code (glob)
  Skipping tests/comprehensive/test-hgsubversion-verify-and-startrev.py it has no-che?k-code (glob)
  Skipping tests/conduithttp.py it has no-che?k-code (glob)
  Skipping tests/fixtures/rsvn.py it has no-che?k-code (glob)
  Skipping tests/test-fb-hgext-remotefilelog-bad-configs.t it has no-che?k-code (glob)
  tests/test-hggit-git-submodules.t:61:
   >   $ grep 'submodule "subrepo2"' -A2 .gitmodules > .gitmodules-new
   don't use grep's context flags
  tests/test-hggit-gitignore.t:124:
   >   $ echo 'foo.*$(?<!bar)' >> .hgignore
   don't use $(expr), use `expr`
  tests/test-hggit-renames.t:79:
   >   $ grep 'submodule "gitsubmodule"' -A2 .gitmodules > .gitmodules-new
   don't use grep's context flags
  Skipping tests/test-hgsql-encoding.t it has no-che?k-code (glob)
  Skipping tests/test-hgsql-race-conditions.t it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-externals.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-branches.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-command-regexes.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-command.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-exec.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-mappings.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-fetch-symlinks.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-push-command.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-push-dirs.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-push-renames.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-single-dir-clone.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-single-dir-push.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-svn-pre-commit-hooks.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-svnwrap.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-tags.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-template-keywords.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-urls.py it has no-che?k-code (glob)
  Skipping tests/test-hgsubversion-utility-commands.py it has no-che?k-code (glob)
  Skipping tests/test_hgsubversion_util.py it has no-che?k-code (glob)
  [1]

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('mercurial/debugcommands.py', 'rb') as fh:
  ...     for line in fh:
  ...         m = re.match("^@command\('([a-z]+)", line)
  ...         if m:
  ...             commands.append(m.group(1))
  >>> scommands = list(sorted(commands))
  >>> for i, command in enumerate(scommands):
  ...     if command != commands[i]:
  ...         print('commands in debugcommands.py not sorted; first differing '
  ...               'command is %s; expected %s' % (commands[i], command))
  ...         break

Prevent adding new files in the root directory accidentally.

  $ testrepohg files 'glob:*'
  .clang-format
  .editorconfig
  .flake8
  .gitignore
  .hg-vendored-crates
  .hgsigs
  .jshintrc
  .watchmanconfig
  CONTRIBUTING
  CONTRIBUTORS
  COPYING
  Makefile
  README.rst
  TARGETS
  hg
  hgeditor
  hgweb.cgi
  setup.py
  vendorcrates.py
