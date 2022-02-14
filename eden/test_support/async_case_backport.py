#!/usr/bin/env python3
#
# Copyright (c) 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008, 2009, 2010,
# 2011, 2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020 Python Software Foundation;
# All Rights Reserved
#
# Licensed under Python Software Foundation License Version 2
# https://spdx.org/licenses/Python-2.0.html
#
# Original version from CPython 3.8
# https://github.com/python/cpython/blob/60c2a810e37994fc640c58d0ef45b6843354b770/Lib/unittest/async_case.py
#
# @nolint
# fmt: off


import asyncio
import inspect

from unittest.case import TestCase



class IsolatedAsyncioTestCase(TestCase):
    # Names intentionally have a long prefix
    # to reduce a chance of clashing with user-defined attributes
    # from inherited test case
    #
    # The class doesn't call loop.run_until_complete(self.setUp()) and family
    # but uses a different approach:
    # 1. create a long-running task that reads self.setUp()
    #    awaitable from queue along with a future
    # 2. await the awaitable object passing in and set the result
    #    into the future object
    # 3. Outer code puts the awaitable and the future object into a queue
    #    with waiting for the future
    # The trick is necessary because every run_until_complete() call
    # creates a new task with embedded ContextVar context.
    # To share contextvars between setUp(), test and tearDown() we need to execute
    # them inside the same task.

    # Note: the test case modifies event loop policy if the policy was not instantiated
    # yet.
    # asyncio.get_event_loop_policy() creates a default policy on demand but never
    # returns None
    # I believe this is not an issue in user level tests but python itself for testing
    # should reset a policy in every test module
    # by calling asyncio.set_event_loop_policy(None) in tearDownModule()

    def __init__(self, methodName: str='runTest') -> None:
        super().__init__(methodName)
        self._asyncioTestLoop = None
        self._asyncioCallsQueue = None

    async def asyncSetUp(self) -> None:
        pass

    async def asyncTearDown(self) -> None:
        pass

    def addAsyncCleanup(self, func, *args, **kwargs) -> None:
        # A trivial trampoline to addCleanup()
        # the function exists because it has a different semantics
        # and signature:
        # addCleanup() accepts regular functions
        # but addAsyncCleanup() accepts coroutines
        #
        # We intentionally don't add inspect.iscoroutinefunction() check
        # for func argument because there is no way
        # to check for async function reliably:
        # 1. It can be "async def func()" iself
        # 2. Class can implement "async def __call__()" method
        # 3. Regular "def func()" that returns awaitable object
        self.addCleanup(*(func, *args), **kwargs)

    def _callSetUp(self) -> None:
        self.setUp()
        self._callAsync(self.asyncSetUp)

    def _callTestMethod(self, method) -> None:
        self._callMaybeAsync(method)

    def _callTearDown(self) -> None:
        self._callAsync(self.asyncTearDown)
        self.tearDown()

    def _callCleanup(self, function, *args, **kwargs) -> None:
        self._callMaybeAsync(function, *args, **kwargs)

    def _callAsync(self, func, *args, **kwargs):
        assert self._asyncioTestLoop is not None
        ret = func(*args, **kwargs)
        assert inspect.isawaitable(ret)
        fut = self._asyncioTestLoop.create_future()
        self._asyncioCallsQueue.put_nowait((fut, ret))
        return self._asyncioTestLoop.run_until_complete(fut)

    def _callMaybeAsync(self, func, *args, **kwargs):
        assert self._asyncioTestLoop is not None
        ret = func(*args, **kwargs)
        if inspect.isawaitable(ret):
            fut = self._asyncioTestLoop.create_future()
            self._asyncioCallsQueue.put_nowait((fut, ret))
            return self._asyncioTestLoop.run_until_complete(fut)
        else:
            return ret

    async def _asyncioLoopRunner(self, fut) -> None:
        self._asyncioCallsQueue = queue = asyncio.Queue()
        fut.set_result(None)
        while True:
            query = await queue.get()
            queue.task_done()
            if query is None:
                return
            fut, awaitable = query
            try:
                ret = await awaitable
                if not fut.cancelled():
                    fut.set_result(ret)
            except asyncio.CancelledError:
                raise
            except Exception as ex:
                if not fut.cancelled():
                    fut.set_exception(ex)

    def _setupAsyncioLoop(self) -> None:
        assert self._asyncioTestLoop is None
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        loop.set_debug(True)
        self._asyncioTestLoop = loop
        fut = loop.create_future()
        # pyre-fixme[16]: `IsolatedAsyncioTestCase` has no attribute
        #  `_asyncioCallsTask`.
        self._asyncioCallsTask = loop.create_task(self._asyncioLoopRunner(fut))
        loop.run_until_complete(fut)

    def _tearDownAsyncioLoop(self) -> None:
        assert self._asyncioTestLoop is not None
        loop = self._asyncioTestLoop
        self._asyncioTestLoop = None
        self._asyncioCallsQueue.put_nowait(None)
        loop.run_until_complete(self._asyncioCallsQueue.join())

        try:
            # cope with Python 3.6
            try:
                asyncio.all_tasks
            except AttributeError:
                return
            # cancel all tasks
            to_cancel = asyncio.all_tasks(loop)
            if not to_cancel:
                return

            for task in to_cancel:
                task.cancel()

            loop.run_until_complete(
                asyncio.gather(*to_cancel, loop=loop, return_exceptions=True))

            for task in to_cancel:
                if task.cancelled():
                    continue
                if task.exception() is not None:
                    loop.call_exception_handler({
                        'message': 'unhandled exception during test shutdown',
                        'exception': task.exception(),
                        'task': task,
                    })
            # shutdown asyncgens
            loop.run_until_complete(loop.shutdown_asyncgens())
        finally:
            asyncio.set_event_loop(None)
            loop.close()

    def run(self, result=None):
        self._setupAsyncioLoop()
        try:
            return super().run(result)
        finally:
            self._tearDownAsyncioLoop()
