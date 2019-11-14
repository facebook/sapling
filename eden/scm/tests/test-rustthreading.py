# Lock tests backported from Python 2.7.15 under PSF license. Adjusted to use
# Rust locks.
#
# Copyright (C) 2001-2018 Python Software Foundation; All Rights reserved
#
# 1. This LICENSE AGREEMENT is between the Python Software Foundation ("PSF"), and
#    the Individual or Organization ("Licensee") accessing and otherwise using Python
#    2.7.15 software in source or binary form and its associated documentation.
#
# 2. Subject to the terms and conditions of this License Agreement, PSF hereby
#    grants Licensee a nonexclusive, royalty-free, world-wide license to reproduce,
#    analyze, test, perform and/or display publicly, prepare derivative works,
#    distribute, and otherwise use Python 2.7.15 alone or in any derivative
#    version, provided, however, that PSF's License Agreement and PSF's notice of
#    copyright, i.e., "Copyright (C) 2001-2018 Python Software Foundation; All Rights
#    Reserved" are retained in Python 2.7.15 alone or in any derivative version
#    prepared by Licensee.
#
# 3. In the event Licensee prepares a derivative work that is based on or
#    incorporates Python 2.7.15 or any part thereof, and wants to make the
#    derivative work available to others as provided herein, then Licensee hereby
#    agrees to include in any such work a brief summary of the changes made to Python
#    2.7.15.
#
# 4. PSF is making Python 2.7.15 available to Licensee on an "AS IS" basis.
#    PSF MAKES NO REPRESENTATIONS OR WARRANTIES, EXPRESS OR IMPLIED.  BY WAY OF
#    EXAMPLE, BUT NOT LIMITATION, PSF MAKES NO AND DISCLAIMS ANY REPRESENTATION OR
#    WARRANTY OF MERCHANTABILITY OR FITNESS FOR ANY PARTICULAR PURPOSE OR THAT THE
#    USE OF PYTHON 2.7.15 WILL NOT INFRINGE ANY THIRD PARTY RIGHTS.
#
# 5. PSF SHALL NOT BE LIABLE TO LICENSEE OR ANY OTHER USERS OF PYTHON 2.7.15
#    FOR ANY INCIDENTAL, SPECIAL, OR CONSEQUENTIAL DAMAGES OR LOSS AS A RESULT OF
#    MODIFYING, DISTRIBUTING, OR OTHERWISE USING PYTHON 2.7.15, OR ANY DERIVATIVE
#    THEREOF, EVEN IF ADVISED OF THE POSSIBILITY THEREOF.
#
# 6. This License Agreement will automatically terminate upon a material breach of
#    its terms and conditions.
#
# 7. Nothing in this License Agreement shall be deemed to create any relationship
#    of agency, partnership, or joint venture between PSF and Licensee.  This License
#    Agreement does not grant permission to use PSF trademarks or trade name in a
#    trademark sense to endorse or promote products or services of Licensee, or any
#    third party.
#
# 8. By copying, installing or otherwise using Python 2.7.15, Licensee agrees
#    to be bound by the terms and conditions of this License Agreement.
#
# no-check-code (imported code)

from __future__ import absolute_import

import os
import thread
import threading
import time
import unittest
from thread import get_ident, start_new_thread

import silenttestrunner
from edenscmnative import threading as rustthreading


# From test_support.py


def threading_setup():
    if thread:
        return (thread._count(),)
    else:
        return (1,)


def threading_cleanup(nb_threads):
    if not thread:
        return

    _MAX_COUNT = 10
    for count in range(_MAX_COUNT):
        n = thread._count()
        if n == nb_threads:
            break
        time.sleep(0.1)
    # XXX print a warning in case of failure?


def reap_children():
    """Use this function at the end of test_main() whenever sub-processes
    are started.  This will help ensure that no extra children (zombies)
    stick around to hog resources and create problems when looking
    for refleaks.
    """

    # Reap all our dead child processes so we don't leave zombies around.
    # These hog resources and might be causing some of the buildbots to die.
    if hasattr(os, "waitpid"):
        any_process = -1
        while True:
            try:
                # This will raise an exception on Windows.  That's ok.
                pid, status = os.waitpid(any_process, os.WNOHANG)
                if pid == 0:
                    break
            except:
                break


# From lock_tests.py


def _wait():
    # A crude wait/yield function not relying on synchronization primitives.
    time.sleep(0.01)


class Bunch(object):
    """
    A bunch of threads.
    """

    def __init__(self, f, n, wait_before_exit=False):
        """
        Construct a bunch of `n` threads running the same function `f`.
        If `wait_before_exit` is True, the threads won't terminate until
        do_finish() is called.
        """
        self.f = f
        self.n = n
        self.started = []
        self.finished = []
        self._can_exit = not wait_before_exit

        def task():
            tid = get_ident()
            self.started.append(tid)
            try:
                f()
            finally:
                self.finished.append(tid)
                while not self._can_exit:
                    _wait()

        try:
            for i in range(n):
                start_new_thread(task, ())
        except:
            self._can_exit = True
            raise

    def wait_for_started(self):
        while len(self.started) < self.n:
            _wait()

    def wait_for_finished(self):
        while len(self.finished) < self.n:
            _wait()

    def do_finish(self):
        self._can_exit = True


class BaseTestCase(unittest.TestCase):
    def setUp(self):
        self._threads = threading_setup()

    def tearDown(self):
        threading_cleanup(*self._threads)
        reap_children()


class BaseLockTests(BaseTestCase):
    """
    Tests for both recursive and non-recursive locks.
    """

    locktype = staticmethod(rustthreading.Condition)

    def test_constructor(self):
        lock = self.locktype()
        del lock

    def test_acquire_destroy(self):
        lock = self.locktype()
        lock.acquire()
        del lock

    def test_acquire_release(self):
        lock = self.locktype()
        lock.acquire()
        lock.release()
        del lock

    def test_try_acquire(self):
        lock = self.locktype()
        self.assertTrue(lock.acquire(False))
        lock.release()

    def test_try_acquire_contended(self):
        lock = self.locktype()
        lock.acquire()
        result = []

        def f():
            result.append(lock.acquire(False))

        Bunch(f, 1).wait_for_finished()
        self.assertFalse(result[0])
        lock.release()

    def test_acquire_contended(self):
        lock = self.locktype()
        lock.acquire()
        N = 5

        def f():
            lock.acquire()
            lock.release()

        b = Bunch(f, N)
        b.wait_for_started()
        _wait()
        self.assertEqual(len(b.finished), 0)
        lock.release()
        b.wait_for_finished()
        self.assertEqual(len(b.finished), N)

    def test_with(self):
        lock = self.locktype()

        def f():
            lock.acquire()
            lock.release()

        def _with(err=None):
            with lock:
                if err is not None:
                    raise err

        _with()
        # Check the lock is unacquired
        Bunch(f, 1).wait_for_finished()
        self.assertRaises(TypeError, _with, TypeError)
        # Check the lock is unacquired
        Bunch(f, 1).wait_for_finished()

    def test_thread_leak(self):
        # The lock shouldn't leak a Thread instance when used from a foreign
        # (non-threading) thread.
        lock = self.locktype()

        def f():
            lock.acquire()
            lock.release()

        n = len(threading.enumerate())
        # We run many threads in the hope that existing threads ids won't
        # be recycled.
        Bunch(f, 15).wait_for_finished()
        self.assertEqual(n, len(threading.enumerate()))


class RLockTests(BaseLockTests):
    """
    Tests for recursive locks.
    """

    locktype = staticmethod(rustthreading.Condition)

    def test_reacquire(self):
        lock = self.locktype()
        lock.acquire()
        lock.acquire()
        lock.release()
        lock.acquire()
        lock.release()
        lock.release()

    def test_release_unacquired(self):
        # Cannot release an unacquired lock
        lock = self.locktype()
        self.assertRaises(RuntimeError, lock.release)
        lock.acquire()
        lock.acquire()
        lock.release()
        lock.acquire()
        lock.release()
        lock.release()
        self.assertRaises(RuntimeError, lock.release)

    def test_different_thread(self):
        # Cannot release from a different thread
        lock = self.locktype()

        def f():
            lock.acquire()

        b = Bunch(f, 1, True)
        try:
            self.assertRaises(RuntimeError, lock.release)
        finally:
            b.do_finish()

    def test__is_owned(self):
        lock = self.locktype()
        self.assertFalse(lock._is_owned())
        lock.acquire()
        self.assertTrue(lock._is_owned())
        lock.acquire()
        self.assertTrue(lock._is_owned())
        result = []

        def f():
            result.append(lock._is_owned())

        Bunch(f, 1).wait_for_finished()
        self.assertFalse(result[0])
        lock.release()
        self.assertTrue(lock._is_owned())
        lock.release()
        self.assertFalse(lock._is_owned())


class ConditionTests(BaseTestCase):
    """
    Tests for condition variables.
    """

    condtype = staticmethod(rustthreading.Condition)

    def test_acquire(self):
        cond = self.condtype()
        # Be default we have an RLock: the condition can be acquired multiple
        # times.
        cond.acquire()
        cond.acquire()
        cond.release()
        cond.release()
        lock = threading.Lock()
        cond = self.condtype(lock)
        cond.acquire()
        self.assertFalse(lock.acquire(False))
        cond.release()
        self.assertTrue(lock.acquire(False))
        self.assertFalse(cond.acquire(False))
        lock.release()
        with cond:
            self.assertFalse(lock.acquire(False))

    def test_unacquired_wait(self):
        cond = self.condtype()
        self.assertRaises(RuntimeError, cond.wait)

    def test_unacquired_notify(self):
        cond = self.condtype()
        self.assertRaises(RuntimeError, cond.notify)

    def _check_notify(self, cond):
        # Note that this test is sensitive to timing.  If the worker threads
        # don't execute in a timely fashion, the main thread may think they
        # are further along then they are.  The main thread therefore issues
        # _wait() statements to try to make sure that it doesn't race ahead
        # of the workers.
        # Secondly, this test assumes that condition variables are not subject
        # to spurious wakeups.  The absence of spurious wakeups is an implementation
        # detail of Condition Cariables in current CPython, but in general, not
        # a guaranteed property of condition variables as a programming
        # construct.  In particular, it is possible that this can no longer
        # be conveniently guaranteed should their implementation ever change.
        N = 5
        ready = []
        results1 = []
        results2 = []
        phase_num = 0

        def f():
            cond.acquire()
            ready.append(phase_num)
            cond.wait()
            cond.release()
            results1.append(phase_num)
            cond.acquire()
            ready.append(phase_num)
            cond.wait()
            cond.release()
            results2.append(phase_num)

        b = Bunch(f, N)
        b.wait_for_started()
        # first wait, to ensure all workers settle into cond.wait() before
        # we continue. See issues #8799 and #30727.
        while len(ready) < 5:
            _wait()
        ready = []
        self.assertEqual(results1, [])
        # Notify 3 threads at first
        cond.acquire()
        cond.notify(3)
        _wait()
        phase_num = 1
        cond.release()
        while len(results1) < 3:
            _wait()
        self.assertEqual(results1, [1] * 3)
        self.assertEqual(results2, [])
        # make sure all awaken workers settle into cond.wait()
        while len(ready) < 3:
            _wait()
        # Notify 5 threads: they might be in their first or second wait
        cond.acquire()
        cond.notify(5)
        _wait()
        phase_num = 2
        cond.release()
        while len(results1) + len(results2) < 8:
            _wait()
        self.assertEqual(results1, [1] * 3 + [2] * 2)
        self.assertEqual(results2, [2] * 3)
        # make sure all workers settle into cond.wait()
        while len(ready) < 5:
            _wait()
        # Notify all threads: they are all in their second wait
        cond.acquire()
        cond.notify_all()
        _wait()
        phase_num = 3
        cond.release()
        while len(results2) < 5:
            _wait()
        self.assertEqual(results1, [1] * 3 + [2] * 2)
        self.assertEqual(results2, [2] * 3 + [3] * 2)
        b.wait_for_finished()

    def test_notify(self):
        cond = self.condtype()
        self._check_notify(cond)
        # A second time, to check internal state is still ok.
        self._check_notify(cond)

    def test_timeout(self):
        cond = self.condtype()
        results = []
        N = 5

        def f():
            cond.acquire()
            t1 = time.time()
            cond.wait(0.2)
            t2 = time.time()
            cond.release()
            results.append(t2 - t1)

        Bunch(f, N).wait_for_finished()
        self.assertEqual(len(results), 5)
        for dt in results:
            self.assertTrue(dt >= 0.2, dt)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
