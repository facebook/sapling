# no-check-code -- see T24862348

from __future__ import absolute_import

import errno
import os
import random
import shutil
import socket
import subprocess
import sys
import time

import test_hgsubversion_util
from edenscm.hgext.hgsubversion import compathacks, util
from edenscm.mercurial import commands, context, hg, node, revlog, util as hgutil


class PushTests(test_hgsubversion_util.TestBase):
    obsolete_mode_tests = True

    def setUp(self):
        test_hgsubversion_util.TestBase.setUp(self)
        self.repo_path = self.load_and_fetch("simple_branch.svndump")[1]

    def test_cant_push_empty_ctx(self):
        repo = self.repo

        def file_callback(repo, memctx, path):
            if path == "adding_file":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError()

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            [],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        old_tip = repo["tip"].node()
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertEqual(tip.node(), old_tip)

    def test_push_add_of_added_upstream_gives_sane_error(self):
        repo = self.repo

        def file_callback(repo, memctx, path):
            if path == "adding_file":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError()

        p1 = repo["default"].node()
        ctx = context.memctx(
            repo,
            (p1, node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        old_tip = repo["tip"].node()
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), old_tip)

        # This node adds the same file as the first one we added, and
        # will be refused by the server for adding a file that already
        # exists. We should respond with an error suggesting the user
        # rebase.
        ctx = context.memctx(
            repo,
            (p1, node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        old_tip = repo["tip"].node()
        try:
            self.pushrevisions()
        except hgutil.Abort as e:
            assert "pull again and rebase" in str(e)
        tip = self.repo["tip"]
        self.assertEqual(tip.node(), old_tip)

    def test_cant_push_with_changes(self):
        repo = self.repo

        def file_callback(repo, memctx, path):
            return compathacks.makememfilectx(
                repo,
                memctx=memctx,
                path=path,
                data="foo",
                islink=False,
                isexec=False,
                copied=False,
            )

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        # Touch an existing file
        repo.wwrite("beta", "something else", "")
        try:
            self.pushrevisions()
        except hgutil.Abort:
            pass
        tip = self.repo["tip"]
        self.assertEqual(new_hash, tip.node())

    def _get_free_address_port(self):
        host = socket.gethostname()
        for attempt in range(5):
            port = random.randint(socket.IPPORT_USERRESERVED, 65535)

            # The `svnserve` binary appears to use the obsolete
            # `gethostbyname(3)` function, which always returns an IPv4
            # address, even on hosts that support and expect IPv6. As a
            # workaround, resolve the hostname within the test harness
            # with `getaddrinfo(3)` to ensure that the client and server
            # both use the same IPv4 or IPv6 address.
            try:
                addrinfo = socket.getaddrinfo(host, port)
            except socket.gaierror:
                # gethostname() can give a hostname that doesn't
                # resolve. Seems bad, but let's fall back to `localhost`
                # in that case and hope for the best.
                host = "localhost"
                addrinfo = socket.getaddrinfo(host, port)
            # On macOS svn seems to have issues with IPv6 at least some
            # of the time, so try and bias towards IPv4. This works
            # because AF_INET is less than AF_INET6 on all platforms
            # I've checked. Hopefully any platform where that's not true
            # will be fine with IPv6 all the time. :)
            selected = sorted(addrinfo)[0]
            # Check the port is free for use by attempting to bind it.
            # `svnserve` sets `SO_REUSEADDR`, so as long as nothing binds the
            # same port in the time between us closing the port and svnserve
            # binding it, we should be fine.
            try:
                s = socket.socket(selected[0], selected[1])
                s.bind((selected[4][0], selected[4][1]))
                s.close()
                return selected
            except Exception:
                pass
        assert False, "Could not get a free port after 5 attempts"

    def internal_push_over_svnserve(self, subdir="", commit=True):
        repo_path = self.load_svndump("simple_branch.svndump")
        open(os.path.join(repo_path, "conf", "svnserve.conf"), "w").write(
            "[general]\nanon-access=write\n[sasl]\n"
        )
        addrinfo = self._get_free_address_port()
        self.host = addrinfo[4][0]
        self.port = addrinfo[4][1]

        # If we're connecting via IPv6 the need to put brackets around the
        # hostname in the URL.
        ipv6 = addrinfo[0] == socket.AF_INET6

        # Ditch any interface information since that's not helpful in
        # a URL
        if ipv6 and ":" in self.host and "%" in self.host:
            self.host = self.host.rsplit("%", 1)[0]

        urlfmt = "svn://[%s]:%d/%s" if ipv6 else "svn://%s:%d/%s"

        args = [
            "svnserve",
            "--daemon",
            "--foreground",
            "--listen-port=%d" % self.port,
            "--listen-host=%s" % self.host,
            "--root=%s" % repo_path,
        ]

        svnserve = subprocess.Popen(
            args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
        self.svnserve_pid = svnserve.pid
        try:
            time.sleep(2)

            shutil.rmtree(self.wc_path)
            commands.clone(
                self.ui(),
                urlfmt % (self.host, self.port, subdir),
                self.wc_path,
                noupdate=True,
            )

            repo = self.repo
            old_tip = repo["tip"].node()
            expected_parent = repo["default"].node()

            def file_callback(repo, memctx, path):
                if path == "adding_file":
                    return compathacks.makememfilectx(
                        repo,
                        memctx=memctx,
                        path=path,
                        data="foo",
                        islink=False,
                        isexec=False,
                        copied=False,
                    )
                raise IOError(errno.EINVAL, "Invalid operation: " + path)

            ctx = context.memctx(
                repo,
                parents=(repo["default"].node(), node.nullid),
                text="automated test",
                files=["adding_file"],
                filectxfn=file_callback,
                user="an_author",
                date="2008-10-07 20:59:48 -0500",
                extra={"branch": "default"},
            )
            repo.commitctx(ctx)
            if not commit:
                return  # some tests use this test as an extended setup.
            hg.update(repo, repo["tip"].node())
            oldauthor = repo["tip"].user()
            commands.push(repo.ui, repo)
            tip = self.repo["tip"]
            self.assertNotEqual(oldauthor, tip.user())
            self.assertNotEqual(tip.node(), old_tip)
            self.assertEqual(tip.parents()[0].node(), expected_parent)
            self.assertEqual(tip["adding_file"].data(), "foo")
            # unintended behaviour:
            self.assertNotEqual("an_author", tip.user())
            self.assertEqual("(no author)", tip.user().rsplit("@", 1)[0])
        finally:
            if sys.version_info >= (2, 6):
                svnserve.kill()
            else:
                test_hgsubversion_util.kill_process(svnserve)

    def test_push_over_svnserve(self):
        self.internal_push_over_svnserve()

    def test_push_over_svnserve_with_subdir(self):
        self.internal_push_over_svnserve(subdir="///branches////the_branch/////")

    def test_push_to_default(self, commit=True):
        repo = self.repo
        old_tip = repo["tip"].node()
        expected_parent = repo["default"].node()

        def file_callback(repo, memctx, path):
            if path == "adding_file":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        if not commit:
            return  # some tests use this test as an extended setup.
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(node.hex(tip.parents()[0].node()), node.hex(expected_parent))
        self.assertEqual(tip["adding_file"].data(), "foo")

    def test_push_two_revs(self):
        # set up some work for us
        self.test_push_to_default(commit=False)
        repo = self.repo
        old_tip = repo["tip"].node()
        expected_parent = repo["tip"].parents()[0].node()

        def file_callback(repo, memctx, path):
            if path == "adding_file2":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo2",
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["adding_file2"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), old_tip)
        self.assertNotEqual(tip.parents()[0].node(), old_tip)
        self.assertEqual(tip.parents()[0].parents()[0].node(), expected_parent)
        self.assertEqual(tip["adding_file2"].data(), "foo2")
        self.assertEqual(tip["adding_file"].data(), "foo")
        self.assertEqual(tip.parents()[0]["adding_file"].data(), "foo")
        try:
            self.assertEqual(tip.parents()[0]["adding_file2"].data(), "foo")
            assert (
                False
            ), "this is impossible, adding_file2 should not be in this manifest."
        except revlog.LookupError:
            pass

    def test_delete_file(self):
        repo = self.repo

        def file_callback(repo, memctx, path):
            return compathacks.filectxfn_deleted(memctx, path)

        old_files = set(repo["default"].manifest().keys())
        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["alpha"],
            file_callback,
            "an author",
            "2008-10-29 21:26:00 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertEqual(old_files, set(tip.manifest().keys() + ["alpha"]))
        self.assert_("alpha" not in tip.manifest())

    def test_push_executable_file(self):
        self.test_push_to_default(commit=True)
        repo = self.repo

        def file_callback(repo, memctx, path):
            if path == "gamma":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=False,
                    isexec=True,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["tip"].node(), node.nullid),
            "message",
            ["gamma"],
            file_callback,
            "author",
            "2008-10-29 21:26:00 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.clean(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), new_hash)
        self.assert_("@" in self.repo["tip"].user())
        self.assertEqual(tip["gamma"].flags(), "x")
        self.assertEqual(tip["gamma"].data(), "foo")
        self.assertEqual(
            sorted([x for x in tip.manifest().keys() if "x" not in tip[x].flags()]),
            ["adding_file", "alpha", "beta"],
        )

    def test_push_symlink_file(self):
        self.test_push_to_default(commit=True)
        repo = self.repo

        def file_callback(repo, memctx, path):
            if path == "gamma":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=True,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["tip"].node(), node.nullid),
            "message",
            ["gamma"],
            file_callback,
            "author",
            "2008-10-29 21:26:00 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        # grab a new repo instance (self.repo is an @property functions)
        repo = self.repo
        tip = repo["tip"]
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip["gamma"].flags(), "l")
        self.assertEqual(tip["gamma"].data(), "foo")
        self.assertEqual(
            sorted([x for x in tip.manifest().keys() if "l" not in tip[x].flags()]),
            ["adding_file", "alpha", "beta"],
        )

        def file_callback2(repo, memctx, path):
            if path == "gamma":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="a" * 129,
                    islink=True,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["tip"].node(), node.nullid),
            "message",
            ["gamma"],
            file_callback2,
            "author",
            "2014-08-08 20:11:41 -0700",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        # grab a new repo instance (self.repo is an @property functions)
        repo = self.repo
        tip = repo["tip"]
        self.assertEqual(tip["gamma"].flags(), "l")
        self.assertEqual(tip["gamma"].data(), "a" * 129)

        def file_callback3(repo, memctx, path):
            if path == "gamma":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="a" * 64 + "b" * 65,
                    islink=True,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["tip"].node(), node.nullid),
            "message",
            ["gamma"],
            file_callback3,
            "author",
            "2014-08-08 20:16:25 -0700",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        repo = self.repo
        tip = repo["tip"]
        self.assertEqual(tip["gamma"].flags(), "l")
        self.assertEqual(tip["gamma"].data(), "a" * 64 + "b" * 65)

    def test_push_existing_file_newly_symlink(self):
        self.test_push_existing_file_newly_execute(
            execute=False, link=True, expected_flags="l"
        )

    def test_push_existing_file_newly_execute(
        self, execute=True, link=False, expected_flags="x"
    ):
        self.test_push_to_default()
        repo = self.repo

        def file_callback(repo, memctx, path):
            return compathacks.makememfilectx(
                repo,
                memctx=memctx,
                path=path,
                data="foo",
                islink=link,
                isexec=execute,
                copied=False,
            )

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "message",
            ["alpha"],
            file_callback,
            "author",
            "2008-1-1 00:00:00 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip["alpha"].data(), "foo")
        self.assertEqual(tip.parents()[0]["alpha"].flags(), "")
        self.assertEqual(tip["alpha"].flags(), expected_flags)
        # while we're here, double check pushing an already-executable file
        # works
        repo = self.repo

        def file_callback2(repo, memctx, path):
            return compathacks.makememfilectx(
                repo,
                memctx=memctx,
                path=path,
                data="bar",
                islink=link,
                isexec=execute,
                copied=False,
            )

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "mutate already-special file alpha",
            ["alpha"],
            file_callback2,
            "author",
            "2008-1-1 00:00:00 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip["alpha"].data(), "bar")
        self.assertEqual(tip.parents()[0]["alpha"].flags(), expected_flags)
        self.assertEqual(tip["alpha"].flags(), expected_flags)
        # now test removing the property entirely
        repo = self.repo

        def file_callback3(repo, memctx, path):
            return compathacks.makememfilectx(
                repo,
                memctx=memctx,
                path=path,
                data="bar",
                islink=False,
                isexec=False,
                copied=False,
            )

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "convert alpha back to regular file",
            ["alpha"],
            file_callback3,
            "author",
            "2008-01-01 00:00:00 -0500",
            {"branch": "default"},
        )
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip["alpha"].data(), "bar")
        self.assertEqual(tip.parents()[0]["alpha"].flags(), expected_flags)
        self.assertEqual(tip["alpha"].flags(), "")

    def test_push_outdated_base_text(self):
        self.test_push_two_revs()
        changes = [("adding_file", "adding_file", "different_content")]
        par = self.repo["tip"].rev()
        self.commitchanges(changes, parent=par)
        self.pushrevisions()
        changes = [("adding_file", "adding_file", "even_more different_content")]
        self.commitchanges(changes, parent=par)
        try:
            self.pushrevisions()
            assert False, "This should have aborted!"
        except hgutil.Abort as e:
            self.assertEqual(
                e.args[0],
                "Outgoing changesets parent is not at subversion "
                "HEAD (svn error 160028)\n"
                "(pull again and rebase on a newer revision)",
            )
            # verify that any pending transactions on the server got cleaned up
            self.assertEqual(
                [],
                os.listdir(
                    os.path.join(self.tmpdir, "testrepo-1", "db", "transactions")
                ),
            )

    def test_push_encoding(self):
        self.test_push_two_revs()
        # Writing then rebasing UTF-8 filenames in a cp1252 windows console
        # used to fail because hg internal encoding was being changed during
        # the interactions with subversion, *and during the rebase*, which
        # confused the dirstate and made it believe the file was deleted.
        fn = "pi\xc3\xa8ce/test"
        changes = [(fn, fn, "a")]
        par = self.repo["tip"].rev()
        self.commitchanges(changes, parent=par)
        self.pushrevisions()

    def test_push_emptying_changeset(self):
        self.repo["tip"]
        changes = [("alpha", None, None), ("beta", None, None)]
        parent = self.repo["tip"].rev()
        self.commitchanges(changes, parent=parent)
        self.pushrevisions()
        self.assertEqual(len(self.repo["tip"].manifest()), 0)

        # Try to re-add a file after emptying the branch
        changes = [("alpha", "alpha", "alpha")]
        self.commitchanges(changes, parent=self.repo["tip"].rev())
        self.pushrevisions()
        self.assertEqual(["alpha"], list(self.repo["tip"].manifest()))

    def test_push_without_pushing_children(self):
        """
        Verify that a push of a nontip node, keeps the tip child
        on top of the pushed commit.
        """

        oldlen = test_hgsubversion_util.repolen(self.repo)
        self.repo["default"].node()

        changes = [("gamma", "gamma", "sometext")]
        newhash1 = self.commitchanges(changes)

        changes = [("delta", "delta", "sometext")]
        self.commitchanges(changes)

        # push only the first commit
        repo = self.repo
        hg.update(repo, newhash1)
        commands.push(repo.ui, repo)
        self.assertEqual(test_hgsubversion_util.repolen(self.repo), oldlen + 2)

        # verify that the first commit is pushed, and the second is not
        commit2 = self.repo["tip"]
        self.assertEqual(commit2.files(), ("delta",))
        self.assertEqual(util.getsvnrev(commit2), None)
        commit1 = commit2.parents()[0]
        self.assertEqual(commit1.files(), ("gamma",))
        prefix = "svn:" + self.repo.svnmeta().uuid
        self.assertEqual(util.getsvnrev(commit1), prefix + "/branches/the_branch@5")

    def test_push_two_that_modify_same_file(self):
        """
        Push performs a rebase if two commits touch the same file.
        This test verifies that code path works.
        """

        oldlen = test_hgsubversion_util.repolen(self.repo)
        self.repo["default"].node()

        changes = [("gamma", "gamma", "sometext")]
        newhash = self.commitchanges(changes)
        changes = [
            ("gamma", "gamma", "sometext\n moretext"),
            ("delta", "delta", "sometext\n moretext"),
        ]
        newhash = self.commitchanges(changes)

        repo = self.repo
        hg.update(repo, newhash)
        commands.push(repo.ui, repo)
        self.assertEqual(test_hgsubversion_util.repolen(self.repo), oldlen + 2)

        # verify that both commits are pushed
        commit1 = self.repo["tip"]
        self.assertEqual(commit1.files(), ("delta", "gamma"))

        prefix = "svn:" + self.repo.svnmeta().uuid
        self.assertEqual(util.getsvnrev(commit1), prefix + "/branches/the_branch@6")
        commit2 = commit1.parents()[0]
        self.assertEqual(commit2.files(), ("gamma",))
        self.assertEqual(util.getsvnrev(commit2), prefix + "/branches/the_branch@5")

    def test_push_in_subdir(self, commit=True):
        repo = self.repo
        old_tip = repo["tip"].node()

        def file_callback(repo, memctx, path):
            if path == "adding_file" or path == "newdir/new_file":
                testData = "fooFirstFile"
                if path == "newdir/new_file":
                    testData = "fooNewFile"
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data=testData,
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2012-12-13 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        p = os.path.join(repo.root, "newdir")
        os.mkdir(p)
        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["newdir/new_file"],
            file_callback,
            "an_author",
            "2012-12-13 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        hg.update(repo, repo["tip"].node())
        self.pushrevisions()
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(tip["adding_file"].data(), "fooFirstFile")
        self.assertEqual(tip["newdir/new_file"].data(), "fooNewFile")

    def test_push_with_rewrite_svncommit(self, commit=True):
        repo = self.repo
        old_tip = repo["tip"].node()
        expected_parent = repo["default"].node()

        def file_callback(repo, memctx, path):
            if path == "adding_file":
                return compathacks.makememfilectx(
                    repo,
                    memctx=memctx,
                    path=path,
                    data="foo",
                    islink=False,
                    isexec=False,
                    copied=False,
                )
            raise IOError(errno.EINVAL, "Invalid operation: " + path)

        ctx = context.memctx(
            repo,
            (repo["default"].node(), node.nullid),
            "automated test",
            ["adding_file"],
            file_callback,
            "an_author",
            "2008-10-07 20:59:48 -0500",
            {"branch": "default"},
        )
        repo.commitctx(ctx)
        if not commit:
            return  # some tests use this test as an extended setup.
        hg.update(repo, repo["tip"].node())
        old_tip_hex = repo["tip"].hex()
        self.pushrevisionswithconfigs(
            configs=[("hgsubversion", "rewritesvncommitwithhghash", True)]
        )
        tip = self.repo["tip"]
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(node.hex(tip.parents()[0].node()), node.hex(expected_parent))
        self.assertEqual(tip["adding_file"].data(), "foo")
        self.assertEqual(
            tip.description(),
            "automated test\nREVERSE_SYNC_ONLY_HG_NODE: " + old_tip_hex,
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
