# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from sapling import httpclient, json, util


urlreq = util.urlreq

# helper class so phabricator_graphql_client can talk using the requests
# third-party library


class PhabricatorClientError(Exception):
    def __init__(self, reason, error):
        Exception.__init__(self, reason, error)


class PhabricatorGraphQLClientRequests:
    def __init__(self, unix_socket_proxy=None):
        self._connection = None
        self._unix_socket_proxy = unix_socket_proxy

    def __verify_connection(self, request_url, timeout):
        urlparts = urlreq.urlparse(request_url)

        if self._connection is None:
            if self._unix_socket_proxy:
                self._connection = httpclient.HTTPConnection(
                    urlparts.hostname,
                    unix_socket_path=self._unix_socket_proxy,
                    timeout=timeout,
                )
            elif urlparts.scheme == "http":
                self._connection = httpclient.HTTPConnection(
                    urlparts.netloc, timeout=timeout
                )
            elif urlparts.scheme == "https":
                self._connection = httpclient.HTTPConnection(
                    urlparts.netloc,
                    timeout=timeout,
                    use_ssl=True,
                )
            else:
                raise PhabricatorClientError("Unknown host scheme: %s", urlparts.scheme)
        return urlparts

    def sendpost(self, request_url, data, timeout, headers=None):
        urlparts = self.__verify_connection(request_url, timeout)
        query = util.urlreq.urlencode(data)

        headers = {
            "Connection": "Keep-Alive",
            "Content-Type": "application/x-www-form-urlencoded",
            **(headers or {}),
        }

        self._connection.request("POST", urlparts.path, query, headers)
        return json.load(self._connection.getresponse())
