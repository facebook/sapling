# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import json

import requests


# helper class so phabricator_graphql_client can talk using the requests
# third-party library


class PhabricatorClientError(Exception):
    def __init__(self, reason, error):
        Exception.__init__(self, reason, error)


class PhabricatorGraphQLClientRequests(object):
    def sendpost(self, request_url, data, timeout, ca_bundle):
        res = requests.post(request_url, data, verify=ca_bundle or True)
        data = json.loads(res.content.decode("utf8"))
        if res.status_code != 200:
            raise PhabricatorClientError(
                "Phabricator not available returned " + str(res.status), res
            )
        # Apparently both singular and plural are used.
        if "error" in data:
            raise PhabricatorClientError("Error in query", data["error"])
        if "errors" in data:
            raise PhabricatorClientError("Error in query", data["errors"])
        return data
