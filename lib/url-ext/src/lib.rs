// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! url-ext: conversions between url::Url and http::Uri
//!
//! Rust has two main types for referring to URLs: the `Uri` type from the
//! `http` crate, and the `Url` type from the `url` crate. `http::Uri` is
//! the canonical URL type used by the Rust HTTP ecosystem. However, it does
//! not have methods for easily constructing an manipulating URLs. As such,
//! if one wants to build a `http::Uri` by joining several URL componenets,
//! one needs to first create a `url::Url`, perform the concatentions, convert
//! back to a string, and then finally re-parse the URL as an `http::Uri`.

use http::Uri;
use url::Url;

pub trait UrlExt {
    fn to_uri(&self) -> Uri;
    fn from_uri(uri: &Uri) -> Self;
}

impl UrlExt for Url {
    fn to_uri(&self) -> Uri {
        self.as_str()
            .parse()
            .expect("url::Url is invalid as http::Uri")
    }

    fn from_uri(uri: &Uri) -> Self {
        Url::parse(&format!("{}", &uri)).expect("http::Uri is invalid as url::Url")
    }
}

pub trait UriExt {
    fn to_url(&self) -> Url;
    fn from_url(url: &Url) -> Self;
}

impl UriExt for Uri {
    fn to_url(&self) -> Url {
        Url::from_uri(self)
    }

    fn from_url(url: &Url) -> Self {
        url.to_uri()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_uri() {
        let original = "https://example.com/";
        let url = Url::parse(original).unwrap();
        let uri = url.to_uri();
        let roundtrip = format!("{}", &uri);
        assert_eq!(original, roundtrip);
    }

    #[test]
    fn from_uri() {
        let original = "https://example.com/";
        let uri = original.parse::<Uri>().unwrap();
        let url = Url::from_uri(&uri);
        let roundtrip = url.as_str();
        assert_eq!(original, roundtrip);
    }

    #[test]
    fn to_url() {
        let original = "https://example.com/";
        let uri = original.parse::<Uri>().unwrap();
        let url = uri.to_url();
        let roundtrip = url.as_str();
        assert_eq!(original, roundtrip);
    }

    #[test]
    fn from_url() {
        let original = "https://example.com/";
        let url = Url::parse(original).unwrap();
        let uri = Uri::from_url(&url);
        let roundtrip = format!("{}", &uri);
        assert_eq!(original, roundtrip);
    }
}
