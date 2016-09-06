#!/usr/bin/env python2.7

import unittest

import silenttestrunner

import ctreemanifest

class FakeStore(object):
    def get(self, xyz):
        return "abcabcabc"

class ctreemanifesttests(unittest.TestCase):
    def testInitialization(self):
        a = ctreemanifest.treemanifest(FakeStore())

if __name__ == '__main__':
    silenttestrunner.main(__name__)
