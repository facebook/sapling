/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::AsRef;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use percent_encoding::percent_decode_str;
use percent_encoding::utf8_percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::NON_ALPHANUMERIC;
use url::Url;

/// Using custom "schemes" from config, resolve given url.
pub fn resolve_custom_scheme(config: &dyn Config, url: Url) -> Result<Url> {
    if let Some(tmpl) = config.get_nonempty("schemes", url.scheme()) {
        let non_scheme = match url.as_str().split_once(':') {
            Some((_, after)) => after.trim_start_matches('/'),
            None => bail!("url {url} has no scheme"),
        };

        let resolved_url = if tmpl.contains("{1}") {
            tmpl.replace("{1}", non_scheme)
        } else {
            format!("{tmpl}{non_scheme}")
        };

        return Url::parse(&resolved_url)
            .with_context(|| format!("parsing resolved custom scheme URL {resolved_url}"));
    }

    Ok(url)
}

pub fn repo_name_from_url(config: &dyn Config, s: &str) -> Option<String> {
    // Use a base_url to support non-absolute urls.
    let base_url = Url::parse("file:///.").unwrap();
    let parse_opts = Url::options().base_url(Some(&base_url));
    match parse_opts.parse(s) {
        Ok(url) => {
            let url = resolve_custom_scheme(config, url).ok()?;

            tracing::trace!("parsed url {}: {:?}", s, url);
            match url.scheme() {
                "mononoke" => {
                    // In Mononoke URLs, the repo name is always the full path
                    // with slashes trimmed.
                    let path = url.path().trim_matches('/');
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                }
                "eager" => {
                    // eager URLs such as eager://C:\some\path don't work with the default
                    // URL logic, so special case to always take the last path component.
                    if let Some((_, path)) = s.split_once(':') {
                        let delims = if cfg!(windows) {
                            &['/', '\\'][..]
                        } else {
                            &['/'][..]
                        };
                        if let Some((_, last)) = path.trim_end_matches(delims).rsplit_once(delims) {
                            if !last.is_empty() {
                                return Some(last.to_string());
                            }
                        }
                    }
                }
                _ => {
                    // Try to remove special prefixes to guess the repo name from that
                    if let Some(repo_prefix) = config.get("remotefilelog", "reponame-path-prefixes")
                    {
                        if let Some((_, reponame)) =
                            url.path().split_once(repo_prefix.to_string().as_str())
                        {
                            if !reponame.is_empty() {
                                return Some(reponame.to_string());
                            }
                        }
                    }
                    // Try the last segment in url path.
                    if let Some(last_segment) = url
                        .path_segments()
                        .and_then(|s| s.rev().find(|s| !s.is_empty()))
                    {
                        return Some(last_segment.to_string());
                    }
                    // Try path. `path_segment` can be `None` for URL like "test:reponame".
                    let path = url.path().trim_matches('/');
                    if !path.is_empty() {
                        return Some(path.to_string());
                    }
                    // Try the hostname. ex. in "fb://fbsource", "fbsource" is a host not a path.
                    // Also see https://www.mercurial-scm.org/repo/hg/help/schemes
                    if let Some(host_str) = url.host_str() {
                        return Some(host_str.to_string());
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("cannot parse url {}: {:?}", s, e);
        }
    }
    None
}

/// All non-alphanumeric characters (except hypens, underscores, and periods)
/// found in the repo's name will be percent-encoded before being used in URLs.
/// Characters allowed in a repo name (like `+` and `/`) since they are reserved
/// characters according to RFC 3986 section 2.2 Reserved Characters (January 2005)
const RESERVED_CHARS: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .add(b'+')
    .add(b'/');

pub fn encode_repo_name(repo_name: impl AsRef<str>) -> String {
    utf8_percent_encode(repo_name.as_ref(), RESERVED_CHARS).to_string()
}

pub fn decode_repo_name(repo_name_encoded: impl AsRef<str>) -> Result<String> {
    Ok(percent_decode_str(repo_name_encoded.as_ref())
        .decode_utf8()
        .context("Repo name must be utf-8 percent encoded")?
        .to_string())
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_repo_name_from_url() {
        let config = BTreeMap::<&str, &str>::from([("schemes.fb", "mononoke://example.com/{1}")]);

        let check = |url, name| {
            assert_eq!(repo_name_from_url(&config, url).as_deref(), name);
        };

        // Ordinary schemes use the basename as the repo name
        check("repo", Some("repo"));
        check("../path/to/repo", Some("repo"));
        check("file:repo", Some("repo"));
        check("file:/path/to/repo", Some("repo"));
        check("file://server/path/to/repo", Some("repo"));
        check("ssh://user@host/repo", Some("repo"));
        check("ssh://user@host/path/to/repo", Some("repo"));
        check("file:/", None);

        // This isn't correct, but is a side-effect of earlier hacks (should
        // be `None`)
        check("ssh://user@host:100/", Some("host"));

        // Mononoke scheme uses the full path, and repo names can contain
        // slashes.
        check("mononoke://example.com/repo", Some("repo"));
        check("mononoke://example.com/path/to/repo", Some("path/to/repo"));
        check("mononoke://example.com/", None);

        // FB scheme uses the full path.
        check("fb:repo", Some("repo"));
        check("fb:path/to/repo", Some("path/to/repo"));
        check("fb:", None);

        // FB scheme works even when there are extra slashes that shouldn't be
        // there.
        check("fb://repo/", Some("repo"));
        check("fb://path/to/repo", Some("path/to/repo"));

        check("eager:/some/repo//", Some("repo"));
        check("eager:///some/repo", Some("repo"));
        if cfg!(windows) {
            check(r"eager:C:\some\repo", Some("repo"));
            check(r"eager:C:\some/repo", Some("repo"));
            check(r"eager://C:\some\repo", Some("repo"));
            check(r"eager://C:\some/repo", Some("repo"));
            check(r"eager:\\C\some\repo", Some("repo"));
        }
    }

    #[test]
    fn test_resolve_custom_scheme() {
        let config = BTreeMap::<&str, &str>::from([
            ("schemes.append", "appended://bar/"),
            ("schemes.subst", "substd://bar/{1}/baz"),
        ]);

        let check = |url, resolved| {
            assert_eq!(
                resolve_custom_scheme(&config, Url::parse(url).unwrap())
                    .unwrap()
                    .as_str(),
                resolved
            );
        };

        check("other://foo", "other://foo");
        check("append:one/two", "appended://bar/one/two");
        check("subst://one/two", "substd://bar/one/two/baz");
    }
}
