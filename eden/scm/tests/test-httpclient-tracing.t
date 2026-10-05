Python HTTP paths and shell-outs carry the provider's trace context.

  $ sl debugpython <<'PY'
  > import bindings
  > from sapling import httpclient, keepalive, url, util
  > class RequestBuilt(Exception):
  >     pass
  > trace_hosts = []
  > def outgoing_trace(host):
  >     trace_hosts.append(host)
  >     return [("cp2", "context")]
  > bindings.clientinfo.outgoing_trace = outgoing_trace
  > bindings.clientinfo.outgoing_trace_env = lambda: [("CONTEXTPROP_BAGGAGE", "child")]
  > captured = {}
  > connection = httpclient.HTTPConnection(b"host.internalfb.com")
  > def capture(_method, _path, folded, _version):
  >     captured.update(folded)
  >     raise RequestBuilt()
  > connection._buildheaders = capture
  > try:
  >     connection.request(b"GET", b"/")
  > except RequestBuilt:
  >     pass
  > assert captured["cp2"] == ("cp2", "context")
  > assert util.shellenviron()["CONTEXTPROP_BAGGAGE"] == "child"
  > captured = {}
  > keepalive.HTTPHandler._start_transaction = (
  >     lambda _self, _connection, request: captured.update(request.unredirected_hdrs)
  > )
  > connection = type("Connection", (), {})()
  > url.httphandler()._start_transaction(
  >     connection, url.urlreq.request("http://host.internalfb.com/")
  > )
  > assert captured == {"cp2": "context"}
  > request = url.urlreq.request("https://host.internalfb.com/")
  > request._tunnel_host = "https://tunnel.internalfb.com:443"
  > handler = url.httphandler()
  > handler.parent = type("Parent", (), {"addheaders": []})()
  > handler._start_transaction(connection, request)
  > assert trace_hosts[-1] == b"tunnel.internalfb.com"
  > print("trace context propagates")
  > PY
  trace context propagates
