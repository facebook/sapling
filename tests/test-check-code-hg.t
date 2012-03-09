  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if ! hg identify -q > /dev/null; then
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi
  $ hg manifest | xargs "$check_code" || echo 'FAILURE IS NOT AN OPTION!!!'

  $ hg manifest | xargs "$check_code" --warnings --nolineno --per-file=0 || true
  contrib/check-code.py:0:
   > #    (r'^\s+[^_ \n][^_. \n]+_[^_\n]+\s*=', "don't use underbars in identifiers"),
   warning: line over 80 characters
  contrib/perf.py:0:
   >         except:
   warning: naked except clause
  contrib/perf.py:0:
   >     #timer(lambda: sum(map(len, repo.dirstate.status(m, [], False, False, False))))
   warning: line over 80 characters
  contrib/perf.py:0:
   >     except:
   warning: naked except clause
  contrib/setup3k.py:0:
   >         except:
   warning: naked except clause
  contrib/setup3k.py:0:
   >     except:
   warning: naked except clause
  contrib/setup3k.py:0:
   > except:
   warning: naked except clause
   warning: naked except clause
   warning: naked except clause
  contrib/shrink-revlog.py:0:
   >                    '(You can delete those files when you are satisfied that your\n'
   warning: line over 80 characters
  contrib/shrink-revlog.py:0:
   >                 ('', 'sort', 'reversepostorder', 'name of sort algorithm to use'),
   warning: line over 80 characters
  contrib/shrink-revlog.py:0:
   >                [('', 'revlog', '', _('index (.i) file of the revlog to shrink')),
   warning: line over 80 characters
  contrib/shrink-revlog.py:0:
   >         except:
   warning: naked except clause
  doc/gendoc.py:0:
   >                "together with Mercurial. Help for other extensions is available "
   warning: line over 80 characters
  hgext/bugzilla.py:0:
   >                 raise util.Abort(_('cannot find bugzilla user id for %s or %s') %
   warning: line over 80 characters
  hgext/bugzilla.py:0:
   >             bzdir = self.ui.config('bugzilla', 'bzdir', '/var/www/html/bugzilla')
   warning: line over 80 characters
  hgext/convert/__init__.py:0:
   >           ('', 'ancestors', '', _('show current changeset in ancestor branches')),
   warning: line over 80 characters
  hgext/convert/bzr.py:0:
   >         except:
   warning: naked except clause
  hgext/convert/common.py:0:
   >             except:
   warning: naked except clause
  hgext/convert/common.py:0:
   >         except:
   warning: naked except clause
   warning: naked except clause
  hgext/convert/convcmd.py:0:
   >         except:
   warning: naked except clause
  hgext/convert/cvs.py:0:
   >                                 # /1 :pserver:user@example.com:2401/cvsroot/foo Ah<Z
   warning: line over 80 characters
  hgext/convert/cvsps.py:0:
   >                     assert len(branches) == 1, 'unknown branch: %s' % e.mergepoint
   warning: line over 80 characters
  hgext/convert/cvsps.py:0:
   >                     ui.write('Ancestors: %s\n' % (','.join(r)))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                     ui.write('Parent: %d\n' % cs.parents[0].id)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                     ui.write('Parents: %s\n' %
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                 except:
   warning: naked except clause
  hgext/convert/cvsps.py:0:
   >                 ui.write('Branchpoints: %s \n' % ', '.join(branchpoints))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Author: %s\n' % cs.author)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Branch: %s\n' % (cs.branch or 'HEAD'))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Date: %s\n' % util.datestr(cs.date,
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Log:\n')
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Members: \n')
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('PatchSet %d \n' % cs.id)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Tag%s: %s \n' % (['', 's'][len(cs.tags) > 1],
   warning: unwrapped ui message
  hgext/convert/git.py:0:
   >             except:
   warning: naked except clause
  hgext/convert/git.py:0:
   >             fh = self.gitopen('git diff-tree --name-only --root -r %s "%s^%s" --'
   warning: line over 80 characters
  hgext/convert/hg.py:0:
   >             # detect missing revlogs and abort on errors or populate self.ignored
   warning: line over 80 characters
  hgext/convert/hg.py:0:
   >             except:
   warning: naked except clause
   warning: naked except clause
  hgext/convert/hg.py:0:
   >         except:
   warning: naked except clause
  hgext/convert/monotone.py:0:
   >             except:
   warning: naked except clause
  hgext/convert/monotone.py:0:
   >         except:
   warning: naked except clause
  hgext/convert/subversion.py:0:
   >                 raise util.Abort(_('svn: branch has no revision %s') % to_revnum)
   warning: line over 80 characters
  hgext/convert/subversion.py:0:
   >             except:
   warning: naked except clause
  hgext/convert/subversion.py:0:
   >         args = [self.baseurl, relpaths, start, end, limit, discover_changed_paths,
   warning: line over 80 characters
  hgext/convert/subversion.py:0:
   >         self.trunkname = self.ui.config('convert', 'svn.trunk', 'trunk').strip('/')
   warning: line over 80 characters
  hgext/convert/subversion.py:0:
   >     except:
   warning: naked except clause
  hgext/convert/subversion.py:0:
   > def get_log_child(fp, url, paths, start, end, limit=0, discover_changed_paths=True,
   warning: line over 80 characters
  hgext/eol.py:0:
   >     if ui.configbool('eol', 'fix-trailing-newline', False) and s and s[-1] != '\n':
   warning: line over 80 characters
   warning: line over 80 characters
  hgext/gpg.py:0:
   >                 except:
   warning: naked except clause
  hgext/hgcia.py:0:
   > except:
   warning: naked except clause
  hgext/hgk.py:0:
   >         ui.write("%s%s\n" % (prefix, description.replace('\n', nlprefix).strip()))
   warning: line over 80 characters
  hgext/hgk.py:0:
   >         ui.write("parent %s\n" % p)
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >         ui.write('k=%s\nv=%s\n' % (name, value))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("author %s %s %s\n" % (ctx.user(), int(date[0]), date[1]))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("branch %s\n\n" % ctx.branch())
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("committer %s %s %s\n" % (committer, int(date[0]), date[1]))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("revision %d\n" % ctx.rev())
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("tree %s\n" % short(ctx.changeset()[0])) # use ctx.node() instead ??
   warning: line over 80 characters
   warning: unwrapped ui message
  hgext/highlight/__init__.py:0:
   >     extensions.wrapfunction(webcommands, '_filerevision', filerevision_highlight)
   warning: line over 80 characters
  hgext/highlight/__init__.py:0:
   >     return ['/* pygments_style = %s */\n\n' % pg_style, fmter.get_style_defs('')]
   warning: line over 80 characters
  hgext/inotify/__init__.py:0:
   >             if self._inotifyon and not ignored and not subrepos and not self._dirty:
   warning: line over 80 characters
  hgext/inotify/server.py:0:
   >                     except:
   warning: naked except clause
  hgext/inotify/server.py:0:
   >             except:
   warning: naked except clause
  hgext/keyword.py:0:
   >     ui.note("hg ci -m '%s'\n" % msg)
   warning: unwrapped ui message
  hgext/mq.py:0:
   >                     raise util.Abort(_("cannot push --exact with applied patches"))
   warning: line over 80 characters
  hgext/mq.py:0:
   >                     raise util.Abort(_("cannot use --exact and --move together"))
   warning: line over 80 characters
  hgext/mq.py:0:
   >                     self.ui.warn(_('Tag %s overrides mq patch of the same name\n')
   warning: line over 80 characters
  hgext/mq.py:0:
   >                 except:
   warning: naked except clause
   warning: naked except clause
  hgext/mq.py:0:
   >             except:
   warning: naked except clause
   warning: naked except clause
   warning: naked except clause
   warning: naked except clause
  hgext/mq.py:0:
   >             raise util.Abort(_('cannot mix -l/--list with options or arguments'))
   warning: line over 80 characters
  hgext/mq.py:0:
   >             raise util.Abort(_('qfold cannot fold already applied patch %s') % p)
   warning: line over 80 characters
  hgext/mq.py:0:
   >           ('', 'move', None, _('reorder patch series and apply only the patch'))],
   warning: line over 80 characters
  hgext/mq.py:0:
   >           ('U', 'noupdate', None, _('do not update the new working directories')),
   warning: line over 80 characters
  hgext/mq.py:0:
   >           ('e', 'exact', None, _('apply the target patch to its recorded parent')),
   warning: line over 80 characters
  hgext/mq.py:0:
   >         except:
   warning: naked except clause
  hgext/mq.py:0:
   >         ui.write("mq:     %s\n" % ', '.join(m))
   warning: unwrapped ui message
  hgext/mq.py:0:
   >     repo.mq.qseries(repo, missing=opts.get('missing'), summary=opts.get('summary'))
   warning: line over 80 characters
  hgext/notify.py:0:
   >                 ui.note(_('notify: suppressing notification for merge %d:%s\n') %
   warning: line over 80 characters
  hgext/patchbomb.py:0:
   >                                                   binnode, seqno=idx, total=total)
   warning: line over 80 characters
  hgext/patchbomb.py:0:
   >             except:
   warning: naked except clause
  hgext/patchbomb.py:0:
   >             ui.write('Subject: %s\n' % subj)
   warning: unwrapped ui message
  hgext/patchbomb.py:0:
   >         p = mail.mimetextpatch('\n'.join(patchlines), 'x-patch', opts.get('test'))
   warning: line over 80 characters
  hgext/patchbomb.py:0:
   >         ui.write('From: %s\n' % sender)
   warning: unwrapped ui message
  hgext/record.py:0:
   >                                   ignoreblanklines=opts.get('ignore_blank_lines'))
   warning: line over 80 characters
  hgext/record.py:0:
   >                                   ignorewsamount=opts.get('ignore_space_change'),
   warning: line over 80 characters
  hgext/zeroconf/__init__.py:0:
   >             publish(name, desc, path, util.getport(u.config("web", "port", 8000)))
   warning: line over 80 characters
  hgext/zeroconf/__init__.py:0:
   >     except:
   warning: naked except clause
   warning: naked except clause
  mercurial/bundlerepo.py:0:
   >       is a bundlerepo for the obtained bundle when the original "other" is remote.
   warning: line over 80 characters
  mercurial/bundlerepo.py:0:
   >     "local" is a local repo from which to obtain the actual incoming changesets; it
   warning: line over 80 characters
  mercurial/bundlerepo.py:0:
   >     tmp = discovery.findcommonincoming(repo, other, heads=onlyheads, force=force)
   warning: line over 80 characters
  mercurial/commands.py:0:
   >                  "     size " + basehdr + "   link     p1     p2       nodeid\n")
   warning: line over 80 characters
  mercurial/commands.py:0:
   >                 raise util.Abort('cannot use localheads with old style discovery')
   warning: line over 80 characters
  mercurial/commands.py:0:
   >                 ui.note('branch %s\n' % data)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.note('node %s\n' % str(data))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.note('tag %s\n' % name)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.write("unpruned common: %s\n" % " ".join([short(n)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 yield 'n', (r, list(set(p for p in cl.parentrevs(r) if p != -1)))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >                 yield 'n', (r, list(set(p for p in rlog.parentrevs(r) if p != -1)))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >             except:
   warning: naked except clause
  mercurial/commands.py:0:
   >             ui.status(_("(run 'hg heads .' to see heads, 'hg merge' to merge)\n"))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >             ui.write("format: id, p1, p2, cset, delta base, len(delta)\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write("local is subset\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write("remote is subset\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('    other            : ' + fmt2 % pcfmt(numoprev, numprev))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >             ui.write('    where prev = p1  : ' + fmt2 % pcfmt(nump1prev, numprev))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >             ui.write('    where prev = p2  : ' + fmt2 % pcfmt(nump2prev, numprev))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >             ui.write('deltas against other : ' + fmt % pcfmt(numother, numdeltas))
   warning: line over 80 characters
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('deltas against p1    : ' + fmt % pcfmt(nump1, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('deltas against p2    : ' + fmt % pcfmt(nump2, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         cmd, ext, mod = extensions.disabledcmd(ui, name, ui.config('ui', 'strict'))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >         except:
   warning: naked except clause
  mercurial/commands.py:0:
   >         revs, checkout = hg.addbranchrevs(repo, other, branches, opts.get('rev'))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >         ui.write("common heads: %s\n" % " ".join([short(n) for n in common]))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write("match: %s\n" % m(d[0]))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('deltas against prev  : ' + fmt % pcfmt(numprev, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('path %s\n' % k)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('uncompressed data size (min/max/avg) : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     Every ID must be a full-length hex node id string. Returns a list of 0s and 1s
   warning: line over 80 characters
  mercurial/commands.py:0:
   >     remoteurl, branches = hg.parseurl(ui.expandpath(remoteurl), opts.get('branch'))
   warning: line over 80 characters
  mercurial/commands.py:0:
   >     ui.write("digraph G {\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write("internal: %s %s\n" % d)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write("standard: %s\n" % util.datestr(d))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('avg chain length  : ' + fmt % avgchainlen)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('case-sensitive: %s\n' % (util.checkcase('.debugfsinfo')
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('compression ratio : ' + fmt % compratio)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('delta size (min/max/avg)             : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('exec: %s\n' % (util.checkexec(path) and 'yes' or 'no'))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('flags  : %s\n' % ', '.join(flags))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('format : %d\n' % format)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('full revision size (min/max/avg)     : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('revision size : ' + fmt2 % totalsize)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('revisions     : ' + fmt2 % numrevs)
   warning: unwrapped ui message
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('symlink: %s\n' % (util.checklink(path) and 'yes' or 'no'))
   warning: unwrapped ui message
  mercurial/commandserver.py:0:
   >         # the ui here is really the repo ui so take its baseui so we don't end up
   warning: line over 80 characters
  mercurial/context.py:0:
   >                 return self._manifestdelta[path], self._manifestdelta.flags(path)
   warning: line over 80 characters
  mercurial/dagparser.py:0:
   >             raise util.Abort(_("invalid character in dag description: %s...") % s)
   warning: line over 80 characters
  mercurial/dagparser.py:0:
   >         >>> dagtext([('n', (0, [-1])), ('C', 'my command line'), ('n', (1, [0]))])
   warning: line over 80 characters
  mercurial/dirstate.py:0:
   >                 if not st is None and not getkind(st.st_mode) in (regkind, lnkkind):
   warning: line over 80 characters
  mercurial/discovery.py:0:
   >     If onlyheads is given, only nodes ancestral to nodes in onlyheads (inclusive)
   warning: line over 80 characters
  mercurial/discovery.py:0:
   > def findcommonoutgoing(repo, other, onlyheads=None, force=False, commoninc=None):
   warning: line over 80 characters
  mercurial/dispatch.py:0:
   >                                                 " (.hg not found)") % os.getcwd())
   warning: line over 80 characters
  mercurial/dispatch.py:0:
   >         aliases, entry = cmdutil.findcmd(cmd, cmdtable, lui.config("ui", "strict"))
   warning: line over 80 characters
  mercurial/dispatch.py:0:
   >         except:
   warning: naked except clause
  mercurial/dispatch.py:0:
   >         return lambda: runcommand(lui, None, cmd, args[:1], ui, options, d, [], {})
   warning: line over 80 characters
  mercurial/dispatch.py:0:
   >     def __init__(self, args, ui=None, repo=None, fin=None, fout=None, ferr=None):
   warning: line over 80 characters
  mercurial/dispatch.py:0:
   >     except:
   warning: naked except clause
  mercurial/hg.py:0:
   >     except:
   warning: naked except clause
  mercurial/hgweb/hgweb_mod.py:0:
   >             self.maxshortchanges = int(self.config("web", "maxshortchanges", 60))
   warning: line over 80 characters
  mercurial/keepalive.py:0:
   >         except:
   warning: naked except clause
  mercurial/keepalive.py:0:
   >     except:
   warning: naked except clause
  mercurial/localrepo.py:0:
   >                         # we return an integer indicating remote head count change
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >                     raise util.Abort(_("empty or missing revlog for %s") % fname)
   warning: line over 80 characters
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >                 if self._tagscache.tagtypes and name in self._tagscache.tagtypes:
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >                 self.hook("precommit", throw=True, parent1=hookp1, parent2=hookp2)
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >             # new requirements = old non-format requirements + new format-related
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >             except:
   warning: naked except clause
  mercurial/localrepo.py:0:
   >         """return status of files between two nodes or node and working directory
   warning: line over 80 characters
  mercurial/localrepo.py:0:
   >         '''Returns a tagscache object that contains various tags related caches.'''
   warning: line over 80 characters
  mercurial/manifest.py:0:
   >             return "".join(struct.pack(">lll", start, end, len(content)) + content
   warning: line over 80 characters
  mercurial/merge.py:0:
   >                 subrepo.submerge(repo, wctx, mctx, wctx.ancestor(mctx), overwrite)
   warning: line over 80 characters
  mercurial/patch.py:0:
   >                  modified, added, removed, copy, getfilectx, opts, losedata, prefix)
   warning: line over 80 characters
  mercurial/patch.py:0:
   >         diffhelpers.addlines(lr, self.hunk, self.lena, self.lenb, self.a, self.b)
   warning: line over 80 characters
  mercurial/patch.py:0:
   >         output.append(_(' %d files changed, %d insertions(+), %d deletions(-)\n')
   warning: line over 80 characters
  mercurial/patch.py:0:
   >     except:
   warning: naked except clause
  mercurial/pure/base85.py:0:
   >             raise OverflowError('Base85 overflow in hunk starting at byte %d' % i)
   warning: line over 80 characters
  mercurial/pure/mpatch.py:0:
   >         frags.extend(reversed(new))                    # what was left at the end
   warning: line over 80 characters
  mercurial/repair.py:0:
   >         except:
   warning: naked except clause
  mercurial/repair.py:0:
   >     except:
   warning: naked except clause
  mercurial/revset.py:0:
   >         elif c.isalnum() or c in '._' or ord(c) > 127: # gather up a symbol/keyword
   warning: line over 80 characters
  mercurial/revset.py:0:
   >     Changesets that are the Nth ancestor (first parents only) of a changeset in set.
   warning: line over 80 characters
  mercurial/scmutil.py:0:
   >                         raise util.Abort(_("path '%s' is inside nested repo %r") %
   warning: line over 80 characters
  mercurial/scmutil.py:0:
   >             "requires features '%s' (upgrade Mercurial)") % "', '".join(missings))
   warning: line over 80 characters
  mercurial/scmutil.py:0:
   >         elif repo.dirstate[abs] != 'r' and (not good or not os.path.lexists(target)
   warning: line over 80 characters
  mercurial/setdiscovery.py:0:
   >     # treat remote heads (and maybe own heads) as a first implicit sample response
   warning: line over 80 characters
  mercurial/setdiscovery.py:0:
   >     undecided = dag.nodeset() # own nodes where I don't know if remote knows them
   warning: line over 80 characters
  mercurial/similar.py:0:
   >         repo.ui.progress(_('searching for similar files'), i, total=len(removed))
   warning: line over 80 characters
  mercurial/simplemerge.py:0:
   >         for zmatch, zend, amatch, aend, bmatch, bend in self.find_sync_regions():
   warning: line over 80 characters
  mercurial/sshrepo.py:0:
   >             self._abort(error.RepoError(_("no suitable response from remote hg")))
   warning: line over 80 characters
  mercurial/sshrepo.py:0:
   >         except:
   warning: naked except clause
  mercurial/subrepo.py:0:
   >                 other, self._repo = hg.clone(self._repo._subparent.ui, {}, other,
   warning: line over 80 characters
  mercurial/subrepo.py:0:
   >         msg = (_(' subrepository sources for %s differ (in checked out version)\n'
   warning: line over 80 characters
  mercurial/transaction.py:0:
   >             except:
   warning: naked except clause
  mercurial/ui.py:0:
   >                 traceback.print_exception(exc[0], exc[1], exc[2], file=self.ferr)
   warning: line over 80 characters
  mercurial/url.py:0:
   >             conn = httpsconnection(host, port, keyfile, certfile, *args, **kwargs)
   warning: line over 80 characters
  mercurial/util.py:0:
   >             except:
   warning: naked except clause
  mercurial/util.py:0:
   >     except:
   warning: naked except clause
  mercurial/verify.py:0:
   >                     except:
   warning: naked except clause
  mercurial/verify.py:0:
   >                 except:
   warning: naked except clause
  mercurial/wireproto.py:0:
   >         # Assuming the future to be filled with the result from the batched request
   warning: line over 80 characters
  mercurial/wireproto.py:0:
   >         '''remote must support _submitbatch(encbatch) and _submitone(op, encargs)'''
   warning: line over 80 characters
  mercurial/wireproto.py:0:
   >     All methods invoked on instances of this class are simply queued and return a
   warning: line over 80 characters
  mercurial/wireproto.py:0:
   >     The decorator returns a function which wraps this coroutine as a plain method,
   warning: line over 80 characters
  setup.py:0:
   >                 raise SystemExit("Python headers are required to build Mercurial")
   warning: line over 80 characters
  setup.py:0:
   >         except:
   warning: naked except clause
  setup.py:0:
   >     # build_py), it will not find osutil & friends, thinking that those modules are
   warning: line over 80 characters
  setup.py:0:
   >     except:
   warning: naked except clause
   warning: naked except clause
  setup.py:0:
   >     isironpython = platform.python_implementation().lower().find("ironpython") != -1
   warning: line over 80 characters
  setup.py:0:
   > except:
   warning: naked except clause
   warning: naked except clause
   warning: naked except clause
  tests/autodiff.py:0:
   >         ui.write('data lost for: %s\n' % fn)
   warning: unwrapped ui message
  tests/run-tests.py:0:
   >     except:
   warning: naked except clause
  tests/test-commandserver.py:0:
   >                         'hooks.pre-identify=python:test-commandserver.hook', 'id'],
   warning: line over 80 characters
  tests/test-commandserver.py:0:
   >     # the cached repo local hgrc contains ui.foo=bar, so showconfig should show it
   warning: line over 80 characters
  tests/test-commandserver.py:0:
   >     print '%c, %r' % (ch, re.sub('encoding: [a-zA-Z0-9-]+', 'encoding: ***', data))
   warning: line over 80 characters
  tests/test-filecache.py:0:
   >     except:
   warning: naked except clause
  tests/test-filecache.py:0:
   > if subprocess.call(['python', '%s/hghave' % os.environ['TESTDIR'], 'cacheable']):
   warning: line over 80 characters
  tests/test-ui-color.py:0:
   > testui.warn('warning\n')
   warning: unwrapped ui message
  tests/test-ui-color.py:0:
   > testui.write('buffered\n')
   warning: unwrapped ui message
  tests/test-walkrepo.py:0:
   >         print "Found %d repositories when I should have found 2" % (len(reposet),)
   warning: line over 80 characters
  tests/test-walkrepo.py:0:
   >         print "Found %d repositories when I should have found 3" % (len(reposet),)
   warning: line over 80 characters
