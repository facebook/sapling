    import sapling.ui, sapling.url

    ui = sapling.ui.ui()
    ui.setconfig("http_proxy", "no", "a.com,*.b.com,.c.com,dd.com")
    ui.setconfig("http_proxy", "host", "example.com")
    p = sapling.url.proxyhandler(ui)
    assert not p.proxy_url("a.com")
    assert not p.proxy_url("a.a.com")
    assert p.proxy_url("aa.com")

    assert not p.proxy_url("b.com")
    assert not p.proxy_url("b.b.com")
    assert p.proxy_url("bb.com")

    assert not p.proxy_url("c.com")
    assert not p.proxy_url("c.c.com")
    assert p.proxy_url("cc.com")

    assert p.proxy_url("d.com")

