/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(mismatched_lifetime_syntaxes)]

use std::collections::HashMap;
use std::iter;
use std::str;
use std::str::FromStr;

use anyhow::Result;
use anyhow::bail;
use bytes::Buf as _;
use bytes::Bytes;
use bytes::BytesMut;
use hex::FromHex;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;
use mononoke_types::path::MPath;
use nom::AsChar as _;
use nom::Err;
use nom::IResult;
use nom::Input as _;
use nom::Needed;
use nom::Parser;
use nom::branch::alt;
use nom::bytes::streaming::tag;
use nom::bytes::streaming::take;
use nom::bytes::streaming::take_while;
use nom::bytes::streaming::take_while1;
use nom::character::streaming::digit1;
use nom::combinator::complete;
use nom::combinator::map;
use nom::combinator::map_res;
use nom::combinator::rest;
use nom::error::ErrorKind;
use nom::error::FromExternalError;
use nom::error::ParseError;
use nom::multi::many0;
use nom::multi::separated_list0;
use nom::sequence::separated_pair;
use nom::sequence::terminated;

use crate::GetbundleArgs;
use crate::GettreepackArgs;
use crate::Request;
use crate::SingleRequest;
use crate::batch;
use crate::errors;

#[derive(Debug, PartialEq)]
pub enum Error {
    Custom(u32),
    BadUtf8,
    BadPath,
    Nom(ErrorKind),
}

impl<E> FromExternalError<&[u8], E> for Error {
    fn from_external_error(input: &[u8], kind: ErrorKind, _e: E) -> Self {
        Self::from_error_kind(input, kind)
    }
}

impl ParseError<&[u8]> for Error {
    fn from_error_kind(_input: &[u8], kind: ErrorKind) -> Self {
        Error::Nom(kind)
    }

    fn append(_input: &[u8], _kind: ErrorKind, other: Self) -> Self {
        other
    }
}

fn take_until1(substr: &str) -> impl Fn(&[u8]) -> IResult<&[u8], &[u8], Error> {
    move |input: &[u8]| {
        use nom::FindSubstring as _;

        match input.find_substring(substr) {
            None => Err(nom::Err::Incomplete(Needed::new(1 + substr.len()))),
            Some(0) => Err(nom::Err::Error(ParseError::from_error_kind(
                input,
                ErrorKind::TakeUntil,
            ))),
            Some(index) => Ok(input.take_split(index)),
        }
    }
}

fn take_until_and_consume1<'a>(
    substr: &str,
) -> impl Parser<&'a [u8], Output = &'a [u8], Error = Error> {
    terminated(take_until1(substr), tag(substr))
}

fn separated_list_complete<'a, F>(
    sep: &str,
    element: F,
) -> impl Parser<&'a [u8], Output = Vec<F::Output>, Error = F::Error>
where
    F: Parser<&'a [u8]>,
{
    separated_list0(complete(tag(sep)), complete(element))
}

fn integer(input: &[u8]) -> IResult<&[u8], usize, Error> {
    map_res(map_res(digit1, str::from_utf8), usize::from_str).parse(input)
}

/// Return an identifier of the form [a-zA-Z_][a-zA-Z0-9_]*. Returns Incomplete
/// if it manages to reach the end of input, as there may be more identifier coming.
fn ident(input: &[u8]) -> IResult<&[u8], &[u8], Error> {
    for (idx, item) in input.iter().enumerate() {
        match *item as char {
            'a'..='z' | 'A'..='Z' | '_' => continue,
            '0'..='9' if idx > 0 => continue,
            _ => {
                if idx > 0 {
                    return Ok((&input[idx..], &input[0..idx]));
                } else {
                    return Err(Err::Error(Error::Nom(ErrorKind::AlphaNumeric)));
                }
            }
        }
    }
    Err(Err::Incomplete(Needed::Unknown))
}

/// As above, but assumes input is complete, so reaching the end of input means
/// the identifier is the entire input.
fn ident_complete(input: &[u8]) -> IResult<&[u8], &[u8], Error> {
    match ident(input) {
        Err(Err::Incomplete(_)) => Ok((b"", input)),
        other => other,
    }
}

// Assumption: input is complete
// We can't use 'integer' defined above as it reads until a non digit character
fn boolean(input: &[u8]) -> IResult<&[u8], bool, Error> {
    map_res(alt((complete(take_while1(u8::is_dec_digit)), rest)), |s| {
        let s = str::from_utf8(s)?;
        anyhow::Ok(u32::from_str(s)? != 0)
    })
    .parse(input)
}

fn batch_param_comma_separated(input: &[u8]) -> IResult<&[u8], Bytes, Error> {
    map_res(terminated(take_while(notcomma), take(1usize)), |k| {
        batch::unescape(k).map(Bytes::from)
    })
    .parse(input)
}

// List of comma-separated values, each of which is encoded using batch param encoding.
fn gettreepack_directories(input: &[u8]) -> IResult<&[u8], Vec<Bytes>, Error> {
    many0(complete(batch_param_comma_separated)).parse(input)
}

// A "*" parameter is a meta-parameter - its argument is a count of
// a number of other parameters. (We accept nested/recursive star parameters,
// but I don't know if that ever happens in practice.)
fn param_star(input: &[u8]) -> IResult<&[u8], HashMap<&[u8], &[u8]>, Error> {
    let (input, _) = tag("* ").parse(input)?;
    let (input, count) = integer(input)?;
    let (input, _) = tag("\n").parse(input)?;
    params_ref(input, count)
}

// A named parameter is a name followed by a decimal integer of the number of
// bytes in the parameter, followed by newline. The parameter value has no terminator.
// ident <bytelen>\n
// <bytelen bytes>
fn param_kv(input: &[u8]) -> IResult<&[u8], HashMap<&[u8], &[u8]>, Error> {
    let (input, key) = ident(input)?;
    let (input, _) = tag(" ").parse(input)?;
    let (input, len) = integer(input)?;
    let (input, _) = tag("\n").parse(input)?;
    let (input, val) = take(len).parse(input)?;
    Ok((input, iter::once((key, val)).collect()))
}

/// Normal ssh protocol params:
/// either a "*", which indicates a number of following parameters,
/// or a named parameter whose value bytes follow.
/// "count" is the number of required parameters, including the "*" parameter - but *not*
/// the parameters that the "*" parameter expands to.
fn params_ref(mut input: &[u8], count: usize) -> IResult<&[u8], HashMap<&[u8], &[u8]>, Error> {
    let mut ret = HashMap::with_capacity(count);

    for _ in 0..count {
        let (rest, val) = alt((param_star, param_kv)).parse(input)?;
        ret.extend(val);
        input = rest;
    }

    Ok((input, ret))
}

fn params(input: &[u8], count: usize) -> IResult<&[u8], HashMap<Vec<u8>, Vec<u8>>, Error> {
    // Parsing of params is down first by extracting references, then converting them to owned
    // Vecs, if successful. This ensures that validating inputs (i.e. making sure we have all the
    // data we need) is not dependent on the length of the arguments, and instead is only dependent
    // on the complexity of what is being parsed (i.e. the count of arguments). This is important
    // because this is hooked into a Tokio decoder, so it'll get called in a loop every time new
    // data is received (e.g. ~8KiB intervals, since that is the buffer size).
    match params_ref(input, count) {
        // Convert to owned if successful.
        Ok((rest, ret)) => {
            let ret = ret
                .into_iter()
                .map(|(k, v)| (k.to_vec(), v.to_vec()))
                .collect();
            Ok((rest, ret))
        }
        // Re-emit errors otherwise
        Err(err) => Err(err),
    }
}

fn notcomma(b: u8) -> bool {
    b != b','
}

// A batch parameter is "name=value", where name ad value are escaped with an ad-hoc
// scheme to protect ',', ';', '=', ':'. The value ends either at the end of the input
// (which is actually from the "batch" command "cmds" parameter), or at a ',', as they're
// comma-delimited.
fn batch_param_escaped(input: &[u8]) -> IResult<&[u8], (Vec<u8>, Vec<u8>), Error> {
    (
        map_res(take_until_and_consume1("="), batch::unescape),
        map_res(alt((complete(take_while(notcomma)), rest)), batch::unescape),
    )
        .parse(input)
}

// Extract parameters from batch - same signature as params
// Batch parameters are a comma-delimited list of parameters; count is unused
// and there's no notion of star params.
fn batch_params(input: &[u8], _count: usize) -> IResult<&[u8], HashMap<Vec<u8>, Vec<u8>>, Error> {
    map(
        separated_list_complete(",", batch_param_escaped),
        HashMap::from_iter,
    )
    .parse(input)
}

// A nodehash is simply 40 hex digits.
fn nodehash(input: &[u8]) -> IResult<&[u8], HgChangesetId, Error> {
    map_res(
        map_res(take(40usize), str::from_utf8),
        HgChangesetId::from_str,
    )
    .parse(input)
}

// A manifestid is simply 40 hex digits.
fn manifestid(input: &[u8]) -> IResult<&[u8], HgManifestId, Error> {
    map_res(
        map_res(take(40usize), str::from_utf8),
        HgManifestId::from_str,
    )
    .parse(input)
}

// A pair of nodehashes, separated by '-'
fn pair(input: &[u8]) -> IResult<&[u8], (HgChangesetId, HgChangesetId), Error> {
    separated_pair(nodehash, tag("-"), nodehash).parse(input)
}

// A space-separated list of pairs.
fn pairlist(input: &[u8]) -> IResult<&[u8], Vec<(HgChangesetId, HgChangesetId)>, Error> {
    separated_list_complete(" ", pair).parse(input)
}

// A space-separated list of changeset IDs
fn hashlist(input: &[u8]) -> IResult<&[u8], Vec<HgChangesetId>, Error> {
    separated_list_complete(" ", nodehash).parse(input)
}

// A changeset is simply 40 hex digits.
fn hg_changeset_id(input: &[u8]) -> IResult<&[u8], HgChangesetId, Error> {
    map_res(
        map_res(take(40usize), str::from_utf8),
        HgChangesetId::from_str,
    )
    .parse(input)
}

// A space-separated list of hg changesets
fn hg_changeset_list(input: &[u8]) -> IResult<&[u8], Vec<HgChangesetId>, Error> {
    separated_list_complete(" ", hg_changeset_id).parse(input)
}

// A space-separated list of manifest IDs
fn manifestlist(input: &[u8]) -> IResult<&[u8], Vec<HgManifestId>, Error> {
    separated_list_complete(" ", manifestid).parse(input)
}

// A space-separated list of strings
fn stringlist(input: &[u8]) -> IResult<&[u8], Vec<String>, Error> {
    separated_list0(
        complete(tag(" ")),
        map_res(
            map_res(
                alt((complete(take_while(u8::is_alphanum)), rest)),
                str::from_utf8,
            ),
            FromStr::from_str,
        ),
    )
    .parse(input)
}

fn hex_stringlist(input: &[u8]) -> IResult<&[u8], Vec<String>, Error> {
    map_res(stringlist, |vs| {
        vs.into_iter()
            .map(|v| {
                Vec::from_hex(v)
                    .map_err(anyhow::Error::from)
                    .and_then(|v| String::from_utf8(v).map_err(anyhow::Error::from))
            })
            .collect::<Result<Vec<String>>>()
    })
    .parse(input)
}

/// A comma-separated list of arbitrary values. The input is assumed to be
/// complete and exact.
fn commavalues(input: &[u8]) -> IResult<&[u8], Vec<Vec<u8>>, Error> {
    if input.is_empty() {
        // Need to handle this separately because the below will return
        // vec![vec![]] on an empty input.
        Ok((b"", vec![]))
    } else {
        Ok((
            b"",
            input
                .split(|c| *c == b',')
                .map(|val| val.to_vec())
                .collect(),
        ))
    }
}

fn notsemi(b: u8) -> bool {
    b != b';'
}

// A command in a batch. Commands are represented as "command parameters". The parameters
// end either at the end of the buffer or at ';'.
fn cmd(input: &[u8]) -> IResult<&[u8], (Vec<u8>, Vec<u8>), Error> {
    let (input, cmd) = take_until_and_consume1(" ").parse(input)?;
    let (input, args) = alt((complete(take_while(notsemi)), rest)).parse(input)?;
    Ok((input, (cmd.to_vec(), args.to_vec())))
}

// A list of batched commands - the list is delimited by ';'.
fn cmdlist(input: &[u8]) -> IResult<&[u8], Vec<(Vec<u8>, Vec<u8>)>, Error> {
    separated_list0(complete(tag(";")), cmd).parse(input)
}

/// Given a hash of parameters, look up a parameter by name, and if it exists,
/// apply a parser to its value. If it doesn't, error out.
fn parseval<'a, F, T>(params: &'a HashMap<Vec<u8>, Vec<u8>>, key: &str, parser: F) -> Result<T>
where
    F: Fn(&'a [u8]) -> IResult<&'a [u8], T, Error>,
{
    match params.get(key.as_bytes()) {
        None => bail!("missing param {}", key),
        Some(v) => match parser(v.as_ref()) {
            Ok((rest, v)) => match rest {
                [] => Ok(v),
                [..] => bail!("Unconsumed characters remain after parsing param"),
            },
            Err(Err::Incomplete(err)) => bail!("param parse incomplete: {:?}", err),
            Err(Err::Error(err) | Err::Failure(err)) => bail!("param parse failed: {:?}", err),
        },
    }
}

/// Given a hash of parameters, look up a parameter by name, and if it exists,
/// apply a parser to its value. If it doesn't, return the default value.
fn parseval_default<'a, F, T>(
    params: &'a HashMap<Vec<u8>, Vec<u8>>,
    key: &str,
    parser: F,
) -> Result<T>
where
    F: Fn(&'a [u8]) -> IResult<&'a [u8], T, Error>,
    T: Default,
{
    match params.get(key.as_bytes()) {
        None => Ok(T::default()),
        Some(v) => match parser(v.as_ref()) {
            Ok((unparsed, v)) => match unparsed {
                [] => Ok(v),
                [..] => bail!(
                    "Unconsumed characters remain after parsing param: {:?}",
                    unparsed
                ),
            },
            Err(Err::Incomplete(err)) => bail!("param parse incomplete: {:?}", err),
            Err(Err::Error(err) | Err::Failure(err)) => bail!("param parse failed: {:?}", err),
        },
    }
}

/// Given a hash of parameters, look up a parameter by name, and if it exists,
/// apply a parser to its value. If it doesn't, return None.
fn parseval_option<'a, F, T>(
    params: &'a HashMap<Vec<u8>, Vec<u8>>,
    key: &str,
    mut parser: F,
) -> Result<Option<T>>
where
    F: Parser<&'a [u8], Output = T, Error = Error>,
{
    match params.get(key.as_bytes()) {
        None => Ok(None),
        Some(v) => match parser.parse(v.as_ref()) {
            Ok((unparsed, v)) => match unparsed {
                [] => Ok(Some(v)),
                [..] => bail!(
                    "Unconsumed characters remain after parsing param: {:?}",
                    unparsed
                ),
            },
            Err(Err::Incomplete(err)) => bail!("param parse incomplete: {:?}", err),
            Err(Err::Error(err) | Err::Failure(err)) => bail!("param parse failed: {:?}", err),
        },
    }
}

/// Parse a command, given some input, a command name (used as a tag), a param parser
/// function (which generalizes over batched and non-batched parameter syntaxes),
/// number of args (since each command has a fixed number of expected parameters,
/// not withstanding '*'), and a function to actually produce a parsed `SingleRequest`.
fn parse_command<'a, C, F, T>(
    cmd: C,
    parse_params: fn(&[u8], usize) -> IResult<&[u8], HashMap<Vec<u8>, Vec<u8>>, Error>,
    nargs: usize,
    func: F,
) -> impl FnMut(&'a [u8]) -> IResult<&'a [u8], T, Error>
where
    F: Fn(HashMap<Vec<u8>, Vec<u8>>) -> Result<T>,
    C: AsRef<[u8]>,
{
    move |input| {
        let (input, _) = tag(cmd.as_ref()).parse(input)?;
        let (input, _) = tag("\n").parse(input)?;
        let (input, v) = parse_params(input, nargs)?;

        match func(v) {
            Ok(t) => Ok((input, t)),
            Err(_e) => Err(Err::Error(Error::Custom(999999))), // ugh
        }
    }
}

/// Parse an ident, and map it to `String`.
fn ident_string(input: &[u8]) -> IResult<&[u8], String, Error> {
    match ident_complete(input) {
        Ok((rest, s)) => Ok((rest, String::from_utf8_lossy(s).into_owned())),
        Err(err) => Err(err),
    }
}

/// Parse utf8 string, assumes that input is complete
fn utf8_string_complete(input: &[u8]) -> IResult<&[u8], String, Error> {
    match String::from_utf8(Vec::from(input)) {
        Ok(s) => Ok((b"", s)),
        Err(_) => Err(Err::Error(Error::BadUtf8)),
    }
}

/// Parse an MPath; assumes that input is complete.
fn path_complete(input: &[u8]) -> IResult<&[u8], MPath, Error> {
    match MPath::new(input) {
        Ok(path) => Ok((b"", path)),
        Err(_) => Err(Err::Error(Error::BadPath)),
    }
}

macro_rules! replace_expr {
    ($_t:tt $sub:expr) => {
        $sub
    };
}

macro_rules! count_tts {
    ($($tts:tt)*) => {0usize $(+ replace_expr!($tts 1usize))*};
}

/// Macro to take a spec of a mercurial wire protocol command and generate the
/// code to invoke a parser for it. This works for "regular" commands with a
/// fixed number of named parameters.
macro_rules! command_common {
    // No parameters
    ($name:expr, $req:ident, $star:expr, $parseparam:expr, { }) => {
        parse_command($name, $parseparam, $star, |_| Ok($req))
    };

    // One key/parser pair for each parameter
    ($name:expr, $req:ident, $star:expr, $parseparam:expr,
            { $( ($key:ident, $parser:expr) )+ }) => {
        parse_command($name, $parseparam, $star+count_tts!( $($key)+ ), |kv| {
            Ok($req {
                $( $key: parseval(&kv, stringify!($key), $parser)?, )*
            })
        })
    };
}

macro_rules! command {
    ($name:expr, $req:ident, $parseparam:expr,
            { $( $key:ident => $parser:expr, )* }) => {
        command_common!($name, $req, 0, $parseparam, { $(($key, $parser))* })
    };
}

macro_rules! command_star {
    ($name:expr, $req:ident, $parseparam:expr,
            { $( $key:ident => $parser:expr, )* }) => {
        command_common!($name, $req, 1, $parseparam, { $(($key, $parser))* })
    };
}

/// Parse a non-batched command
fn parse_singlerequest(input: &[u8]) -> IResult<&[u8], SingleRequest, Error> {
    parse_with_params(input, params)
}

struct Batch {
    cmds: Vec<(Vec<u8>, Vec<u8>)>,
}

fn parse_batchrequest(input: &[u8]) -> IResult<&[u8], Vec<SingleRequest>, Error> {
    fn parse_cmd(input: &[u8]) -> IResult<&[u8], SingleRequest, Error> {
        parse_with_params(input, batch_params)
    }

    let (rest, batch) = command_star!("batch", Batch, params, {
        cmds => cmdlist,
    })
    .parse(input)?;

    let mut parsed_cmds = Vec::with_capacity(batch.cmds.len());
    for cmd in batch.cmds {
        let full_cmd = Bytes::from([cmd.0, cmd.1].join(&b'\n'));
        let ([], cmd) = complete(parse_cmd).parse(&full_cmd)? else {
            return Err(Err::Error(Error::Nom(ErrorKind::Eof)));
        };
        parsed_cmds.push(cmd);
    }
    Ok((rest, parsed_cmds))
}

pub fn parse_request(buf: &mut BytesMut) -> Result<Option<Request>> {
    let res = alt((
        map(parse_batchrequest, Request::Batch),
        map(parse_singlerequest, Request::Single),
    ))
    .parse(buf);

    match res {
        Ok((rest, val)) => {
            buf.advance(buf.len() - rest.len());
            Ok(Some(val))
        }
        Err(Err::Incomplete(_)) => Ok(None),
        Err(Err::Error(err) | Err::Failure(err)) => {
            println!("parse_request parsing error: {:?}", err);
            bail!(errors::ErrorKind::CommandParse(
                String::from_utf8_lossy(buf.as_ref()).into_owned(),
            ));
        }
    }
}

/// Common parser, generalized over how to parse parameters (either unbatched or
/// batched syntax.)
#[rustfmt::skip]
fn parse_with_params(
    input: &[u8],
    parse_params: fn(&[u8], usize)
        -> IResult<&[u8], HashMap<Vec<u8>, Vec<u8>>, Error>,
) -> IResult<&[u8], SingleRequest, Error> {
    use SingleRequest::*;

    alt((
        command!("between", Between, parse_params, {
            pairs => pairlist,
        }),
        command!("branchmap", Branchmap, parse_params, {}),
        command!("capabilities", Capabilities, parse_params, {}),
        parse_command("debugwireargs", parse_params, 2+1, |kv| {
            Ok(Debugwireargs {
                one: parseval(&kv, "one", ident_complete)?.to_vec(),
                two: parseval(&kv, "two", ident_complete)?.to_vec(),
                all_args: kv,
            })
        }),
        parse_command("clienttelemetry", parse_params, 1, |kv| {
            Ok(ClientTelemetry{
                args: kv,
            })
        }),
        parse_command("getbundle", parse_params, 1, |kv| {
            Ok(Getbundle(GetbundleArgs {
                // Some params are currently ignored, like:
                // - obsmarkers
                // - cg
                // - cbattempted
                // If those params are needed, they should be parsed here.
                heads: parseval_default(&kv, "heads", hashlist)?,
                common: parseval_default(&kv, "common", hashlist)?,
                bundlecaps: parseval_default(&kv, "bundlecaps", commavalues)?.into_iter().collect(),
                listkeys: parseval_default(&kv, "listkeys", commavalues)?,
                phases: parseval_default(&kv, "phases", boolean)?,
            }))
        }),
        command!("heads", Heads, parse_params, {}),
        command!("hello", Hello, parse_params, {}),
        command!("listkeys", Listkeys, parse_params, {
            namespace => ident_string,
        }),
        command!("listkeyspatterns", ListKeysPatterns, parse_params, {
            namespace => ident_string,
            patterns => hex_stringlist,
        }),
        command!("lookup", Lookup, parse_params, {
            key => utf8_string_complete,
        }),
        command_star!("known", Known, parse_params, {
            nodes => hashlist,
        }),
        command_star!("knownnodes", Knownnodes, parse_params, {
            nodes => hg_changeset_list,
        }),
        command!("unbundle", Unbundle, parse_params, {
            heads => stringlist,
        }),
        command!("unbundlereplay", UnbundleReplay, parse_params, {
            heads => stringlist,
            replaydata => utf8_string_complete,
            respondlightly => boolean,
        }),
        parse_command("gettreepack", parse_params, 1, |kv| {
            Ok(Gettreepack(GettreepackArgs {
                rootdir: parseval(&kv, "rootdir", path_complete)?,
                mfnodes: parseval(&kv, "mfnodes", manifestlist)?,
                basemfnodes: parseval(&kv, "basemfnodes", manifestlist)?.into_iter().collect(),
                directories: parseval(&kv, "directories", gettreepack_directories)?,
                depth: parseval_option(&kv, "depth", map_res(
                    map_res(alt((complete(take_while1(u8::is_dec_digit)), rest)), str::from_utf8),
                    usize::from_str
                ))?,
            }))
        }),
        parse_command("stream_out_shallow", parse_params, 1, |kv| {
            Ok(StreamOutShallow {
                tag: parseval_option(&kv, "tag", utf8_string_complete)?
            })
        }),
        command_star!("getpackv1", GetpackV1, parse_params, {}),
        command_star!("getpackv2", GetpackV2, parse_params, {}),
        command!("getcommitdata", GetCommitData, parse_params, {
            nodes => hg_changeset_list,
        }),
    )).parse(input)
}

/// Test individual combinators
#[cfg(test)]
mod test {
    use maplit::hashmap;
    use mercurial_types_mocks::nodehash::NULL_HASH;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_integer() {
        assert_eq!(integer(b"1234 "), Ok((&b" "[..], 1234)));
        assert_eq!(integer(b"1234"), Err(Err::Incomplete(Needed::new(1))));
    }

    #[mononoke::test]
    fn test_ident() {
        let input = b"1234 ".as_slice();
        assert_eq!(
            ident(input),
            Err(Err::Error(Error::Nom(ErrorKind::AlphaNumeric))),
        );

        let input = b" 1234 ".as_slice();
        assert_eq!(
            ident(input),
            Err(Err::Error(Error::Nom(ErrorKind::AlphaNumeric))),
        );

        assert_eq!(ident(b"foo"), Err(Err::Incomplete(Needed::Unknown)));

        assert_eq!(ident(b"foo "), Ok((&b" "[..], &b"foo"[..])));
    }

    #[mononoke::test]
    fn test_param_star() {
        let p = b"* 0\ntrailer";
        assert_eq!(param_star(p), Ok((&b"trailer"[..], hashmap! {})));

        let p = b"* 1\n\
                  foo 12\n\
                  hello world!trailer";
        assert_eq!(
            param_star(p),
            Ok((
                &b"trailer"[..],
                hashmap! {
                    b"foo".as_ref() => b"hello world!".as_ref(),
                }
            )),
        );

        let p = b"* 2\n\
                  foo 12\n\
                  hello world!\
                  bar 4\n\
                  bloptrailer";
        assert_eq!(
            param_star(p),
            Ok((
                &b"trailer"[..],
                hashmap! {
                    b"foo".as_ref() => b"hello world!".as_ref(),
                    b"bar".as_ref() => b"blop".as_ref(),
                }
            )),
        );

        // no trailer
        let p = b"* 0\n";
        assert_eq!(param_star(p), Ok((&b""[..], hashmap! {})));

        let p = b"* 1\n\
                  foo 12\n\
                  hello world!";
        assert_eq!(
            param_star(p),
            Ok((
                &b""[..],
                hashmap! {
                    b"foo".as_ref() => b"hello world!".as_ref(),
                }
            )),
        );
    }

    #[mononoke::test]
    fn test_param_kv() {
        let p = b"foo 12\n\
                  hello world!trailer";
        assert_eq!(
            param_kv(p),
            Ok((
                &b"trailer"[..],
                hashmap! {
                    b"foo".as_ref() => b"hello world!".as_ref(),
                }
            )),
        );

        let p = b"foo 12\n\
                  hello world!";
        assert_eq!(
            param_kv(p),
            Ok((
                &b""[..],
                hashmap! {
                    b"foo".as_ref() => b"hello world!".as_ref(),
                }
            )),
        );
    }

    #[mononoke::test]
    fn test_params() {
        let p = b"bar 12\n\
                  hello world!\
                  foo 7\n\
                  blibble\
                  very_long_key_no_data 0\n\
                  is_ok 1\n\
                  y\n\
                  badly formatted thing ";

        match params(p, 1) {
            Ok((_, v)) => assert_eq!(
                v,
                hashmap! {
                    b"bar".to_vec() => b"hello world!".to_vec(),
                }
            ),
            Err(bad) => panic!("bad result {:?}", bad),
        }

        match params(p, 2) {
            Ok((_, v)) => assert_eq!(
                v,
                hashmap! {
                    b"bar".to_vec() => b"hello world!".to_vec(),
                    b"foo".to_vec() => b"blibble".to_vec(),
                }
            ),
            Err(bad) => panic!("bad result {:?}", bad),
        }

        match params(p, 4) {
            Ok((b"\nbadly formatted thing ", v)) => assert_eq!(
                v,
                hashmap! {
                    b"bar".to_vec() => b"hello world!".to_vec(),
                    b"foo".to_vec() => b"blibble".to_vec(),
                    b"very_long_key_no_data".to_vec() => b"".to_vec(),
                    b"is_ok".to_vec() => b"y".to_vec(),
                }
            ),
            bad => panic!("bad result {:?}", bad),
        }

        match params(p, 5) {
            Err(Err::Error(Error::Nom(ErrorKind::AlphaNumeric))) => {}
            bad => panic!("bad result {:?}", bad),
        }

        match params(&p[..3], 1) {
            Err(Err::Incomplete(_)) => {}
            bad => panic!("bad result {:?}", bad),
        }

        for l in 0..p.len() {
            match params(&p[..l], 4) {
                Err(Err::Incomplete(_)) => {}
                Ok((remain, ref kv)) => {
                    assert_eq!(kv.len(), 4);
                    assert!(
                        b"\nbadly formatted thing ".starts_with(remain),
                        "remain \"{:?}\"",
                        remain
                    );
                }
                bad => panic!("bad result l {} bad {:?}", l, bad),
            }
        }
    }

    #[mononoke::test]
    fn test_params_star() {
        let star = b"* 1\n\
                     foo 0\n\
                     bar 0\n";
        match params(star, 2) {
            Err(Err::Incomplete(_)) => panic!("unexpectedly incomplete"),
            Ok((remain, kv)) => {
                assert_eq!(remain, b"");
                assert_eq!(
                    kv,
                    hashmap! {
                        b"foo".to_vec() => vec!{},
                        b"bar".to_vec() => vec!{},
                    }
                );
            }
            Err(Err::Error(err) | Err::Failure(err)) => panic!("unexpected error {:?}", err),
        }

        let star = b"* 2\n\
                     foo 0\n\
                     plugh 0\n\
                     bar 0\n";
        match params(star, 2) {
            Err(Err::Incomplete(_)) => panic!("unexpectedly incomplete"),
            Ok((remain, kv)) => {
                assert_eq!(remain, b"");
                assert_eq!(
                    kv,
                    hashmap! {
                        b"foo".to_vec() => vec!{},
                        b"bar".to_vec() => vec!{},
                        b"plugh".to_vec() => vec!{},
                    }
                );
            }
            Err(Err::Error(err) | Err::Failure(err)) => panic!("unexpected error {:?}", err),
        }

        let star = b"* 0\n\
                     bar 0\n";
        match params(star, 2) {
            Err(Err::Incomplete(_)) => panic!("unexpectedly incomplete"),
            Ok((remain, kv)) => {
                assert_eq!(remain, b"");
                assert_eq!(
                    kv,
                    hashmap! {
                        b"bar".to_vec() => vec!{},
                    }
                );
            }
            Err(Err::Error(err) | Err::Failure(err)) => panic!("unexpected error {:?}", err),
        }

        match params(&star[..4], 2) {
            Err(Err::Incomplete(_)) => {}
            Ok((remain, kv)) => panic!("unexpected Done remain {:?} kv {:?}", remain, kv),
            Err(Err::Error(err) | Err::Failure(err)) => panic!("unexpected error {:?}", err),
        }
    }

    #[mononoke::test]
    fn test_batch_param_escaped() {
        let p = b"foo=b:ear";

        assert_eq!(
            batch_param_escaped(p),
            Ok((&b""[..], (b"foo".to_vec(), b"b=ar".to_vec()))),
        );
    }

    #[mononoke::test]
    fn test_batch_params() {
        let p = b"foo=bar";

        assert_eq!(
            batch_params(p, 0),
            Ok((
                &b""[..],
                hashmap! {
                    b"foo".to_vec() => b"bar".to_vec(),
                }
            )),
        );

        let p = b"foo=bar,biff=bop,esc:c:o:s:e=esc:c:o:s:e";

        assert_eq!(
            batch_params(p, 0),
            Ok((
                &b""[..],
                hashmap! {
                    b"foo".to_vec() => b"bar".to_vec(),
                    b"biff".to_vec() => b"bop".to_vec(),
                    b"esc:,;=".to_vec() => b"esc:,;=".to_vec(),
                }
            )),
        );

        let p = b"";

        assert_eq!(batch_params(p, 0), Ok((&b""[..], hashmap! {})));

        let p = b"foo=";

        assert_eq!(
            batch_params(p, 0),
            Ok((&b""[..], hashmap! {b"foo".to_vec() => b"".to_vec()})),
        );
    }

    #[mononoke::test]
    fn test_nodehash() {
        assert_eq!(
            nodehash(b"0000000000000000000000000000000000000000"),
            Ok((&b""[..], HgChangesetId::new(NULL_HASH))),
        );

        let input = b"000000000000000000000000000000x000000000".as_slice();
        assert_eq!(
            nodehash(input),
            Err(Err::Error(Error::Nom(ErrorKind::MapRes))),
        );

        assert_eq!(
            nodehash(b"000000000000000000000000000000000000000"),
            Err(Err::Incomplete(Needed::new(1))),
        );
    }

    #[mononoke::test]
    fn test_parseval_extra_characters() {
        let kv = hashmap! {
        b"foo".to_vec() => b"0000000000000000000000000000000000000000extra".to_vec(),
        };
        match parseval(&kv, "foo", hashlist) {
            Err(_) => {}
            _ => panic!(
                "Paramval parse failed: Did not raise an error for param\
                 with trailing characters."
            ),
        }
    }

    #[mononoke::test]
    fn test_parseval_default_extra_characters() {
        let kv = hashmap! {
        b"foo".to_vec() => b"0000000000000000000000000000000000000000extra".to_vec(),
        };
        match parseval_default(&kv, "foo", hashlist) {
            Err(_) => {}
            _ => panic!(
                "paramval_default parse failed: Did not raise an error for param\
                 with trailing characters."
            ),
        }
    }

    #[mononoke::test]
    fn test_pair() {
        let p =
            b"0000000000000000000000000000000000000000-0000000000000000000000000000000000000000";
        assert_eq!(
            pair(p),
            Ok((
                &b""[..],
                (HgChangesetId::new(NULL_HASH), HgChangesetId::new(NULL_HASH))
            )),
        );

        assert_eq!(pair(&p[..80]), Err(Err::Incomplete(Needed::new(1))));

        assert_eq!(pair(&p[..41]), Err(Err::Incomplete(Needed::new(40))));

        assert_eq!(pair(&p[..40]), Err(Err::Incomplete(Needed::new(1))));
    }

    #[mononoke::test]
    fn test_pairlist() {
        let p =
            b"0000000000000000000000000000000000000000-0000000000000000000000000000000000000000 \
              0000000000000000000000000000000000000000-0000000000000000000000000000000000000000";
        assert_eq!(
            pairlist(p),
            Ok((
                &b""[..],
                vec![
                    (HgChangesetId::new(NULL_HASH), HgChangesetId::new(NULL_HASH)),
                    (HgChangesetId::new(NULL_HASH), HgChangesetId::new(NULL_HASH))
                ]
            )),
        );

        let p =
            b"0000000000000000000000000000000000000000-0000000000000000000000000000000000000000";
        assert_eq!(
            pairlist(p),
            Ok((
                &b""[..],
                vec![(HgChangesetId::new(NULL_HASH), HgChangesetId::new(NULL_HASH))]
            )),
        );

        let p = b"";
        assert_eq!(pairlist(p), Ok((&b""[..], vec![])));

        let p = b"0000000000000000000000000000000000000000-00000000000000";
        assert_eq!(
            pairlist(p),
            Ok((
                &b"0000000000000000000000000000000000000000-00000000000000"[..],
                vec![]
            )),
        );
    }

    #[mononoke::test]
    fn test_hashlist() {
        let p =
            b"0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 \
              0000000000000000000000000000000000000000 0000000000000000000000000000000000000000";
        assert_eq!(
            hashlist(p),
            Ok((
                &b""[..],
                vec![
                    HgChangesetId::new(NULL_HASH),
                    HgChangesetId::new(NULL_HASH),
                    HgChangesetId::new(NULL_HASH),
                    HgChangesetId::new(NULL_HASH)
                ]
            )),
        );

        let p = b"0000000000000000000000000000000000000000";
        assert_eq!(
            hashlist(p),
            Ok((&b""[..], vec![HgChangesetId::new(NULL_HASH)])),
        );

        let p = b"";
        assert_eq!(hashlist(p), Ok((&b""[..], vec![])));

        // incomplete should leave bytes on the wire
        let p = b"00000000000000000000000000000";
        assert_eq!(
            hashlist(p),
            Ok((&b"00000000000000000000000000000"[..], vec![])),
        );
    }

    #[mononoke::test]
    fn test_commavalues() {
        // Empty list
        let p = b"";
        assert_eq!(commavalues(p), Ok((&b""[..], vec![])));

        // Single entry
        let p = b"abc";
        assert_eq!(commavalues(p), Ok((&b""[..], vec![b"abc".to_vec()])));

        // Multiple entries
        let p = b"123,abc,test,456";
        assert_eq!(
            commavalues(p),
            Ok((
                &b""[..],
                vec![
                    b"123".to_vec(),
                    b"abc".to_vec(),
                    b"test".to_vec(),
                    b"456".to_vec(),
                ]
            )),
        );
    }

    #[mononoke::test]
    fn test_cmd() {
        let p = b"foo bar";

        assert_eq!(cmd(p), Ok((&b""[..], (b"foo".to_vec(), b"bar".to_vec()))));

        let p = b"noparam ";
        assert_eq!(cmd(p), Ok((&b""[..], (b"noparam".to_vec(), b"".to_vec()))));
    }

    #[mononoke::test]
    fn test_cmdlist() {
        let p = b"foo bar";

        assert_eq!(
            cmdlist(p),
            Ok((&b""[..], vec![(b"foo".to_vec(), b"bar".to_vec())])),
        );

        let p = b"foo bar;biff blop";

        assert_eq!(
            cmdlist(p),
            Ok((
                &b""[..],
                vec![
                    (b"foo".to_vec(), b"bar".to_vec()),
                    (b"biff".to_vec(), b"blop".to_vec()),
                ],
            )),
        );
    }
}

/// Test parsing each command
#[cfg(test)]
mod test_parse {
    use std::fmt::Debug;

    use maplit::btreeset;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mononoke_types::path::MPath;

    use super::*;

    fn hash_ones() -> HgChangesetId {
        HgChangesetId::new("1111111111111111111111111111111111111111".parse().unwrap())
    }

    fn hash_twos() -> HgChangesetId {
        HgChangesetId::new("2222222222222222222222222222222222222222".parse().unwrap())
    }

    fn hash_threes() -> HgChangesetId {
        HgChangesetId::new("3333333333333333333333333333333333333333".parse().unwrap())
    }

    fn hash_fours() -> HgChangesetId {
        HgChangesetId::new("4444444444444444444444444444444444444444".parse().unwrap())
    }

    fn hash_ones_manifest() -> HgManifestId {
        HgManifestId::new("1111111111111111111111111111111111111111".parse().unwrap())
    }

    fn hash_twos_manifest() -> HgManifestId {
        HgManifestId::new("2222222222222222222222222222222222222222".parse().unwrap())
    }

    /// Common code for testing parsing:
    /// - check all truncated inputs return "Ok(None)"
    /// - complete inputs return the expected result, and leave any remainder in
    ///   the input buffer.
    fn test_parse<I: AsRef<[u8]> + Debug>(inp: I, exp: Request) {
        test_parse_with_extra(inp, exp, b"extra")
    }

    fn test_parse_with_extra<I>(inp: I, exp: Request, extra: &[u8])
    where
        I: AsRef<[u8]> + Debug,
    {
        let inbytes = inp.as_ref();

        // check for short inputs
        for l in 0..inbytes.len() - 1 {
            let mut buf = BytesMut::from(&inbytes[0..l]);
            match parse_request(&mut buf) {
                Ok(None) => {}
                Ok(Some(val)) => panic!(
                    "BAD PASS: inp >>{:?}<< lpassed unexpectedly val {:?} pass with {}/{} bytes",
                    Bytes::from(inbytes.to_vec()),
                    val,
                    l,
                    inbytes.len()
                ),
                Err(err) => panic!(
                    "BAD FAIL: inp >>{:?}<< failed {:?} (not incomplete) with {}/{} bytes",
                    Bytes::from(inbytes.to_vec()),
                    err,
                    l,
                    inbytes.len()
                ),
            };
        }

        // check for exact and extra
        for l in 0..extra.len() {
            let mut buf = BytesMut::from(inbytes);
            buf.extend_from_slice(&extra[0..l]);
            let buflen = buf.len();
            match parse_request(&mut buf) {
                Ok(Some(val)) => assert_eq!(val, exp, "with {}/{} bytes", buflen, inbytes.len()),
                Ok(None) => panic!(
                    "BAD INCOMPLETE: inp >>{:?}<< extra {} incomplete {}/{} bytes",
                    Bytes::from(inbytes.to_vec()),
                    l,
                    buflen,
                    inbytes.len()
                ),
                Err(err) => panic!(
                    "BAD FAIL: inp >>{:?}<< extra {} failed {:?} (not incomplete) with {}/{} bytes",
                    Bytes::from(inbytes.to_vec()),
                    l,
                    err,
                    buflen,
                    inbytes.len()
                ),
            };
            assert_eq!(&*buf, &extra[0..l]);
        }
    }

    #[mononoke::test]
    fn test_parse_batch_1() {
        let inp = "batch\n\
                   * 0\n\
                   cmds 6\n\
                   hello ";

        test_parse(inp, Request::Batch(vec![SingleRequest::Hello]))
    }

    #[mononoke::test]
    fn test_parse_batch_2() {
        let inp = "batch\n\
                   * 0\n\
                   cmds 12\n\
                   known nodes=";

        test_parse(
            inp,
            Request::Batch(vec![SingleRequest::Known { nodes: vec![] }]),
        )
    }

    #[mononoke::test]
    fn test_parse_batch_3() {
        let inp = "batch\n\
                   * 0\n\
                   cmds 19\n\
                   hello ;known nodes=";

        test_parse(
            inp,
            Request::Batch(vec![
                SingleRequest::Hello,
                SingleRequest::Known { nodes: vec![] },
            ]),
        )
    }

    #[mononoke::test]
    fn test_parse_between() {
        let inp = "between\n\
             pairs 163\n\
             1111111111111111111111111111111111111111-2222222222222222222222222222222222222222 \
             3333333333333333333333333333333333333333-4444444444444444444444444444444444444444";
        test_parse(
            inp,
            Request::Single(SingleRequest::Between {
                pairs: vec![(hash_ones(), hash_twos()), (hash_threes(), hash_fours())],
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_branchmap() {
        let inp = "branchmap\n";

        test_parse(inp, Request::Single(SingleRequest::Branchmap {}));
    }

    #[mononoke::test]
    fn test_parse_capabilities() {
        let inp = "capabilities\n";

        test_parse(inp, Request::Single(SingleRequest::Capabilities {}));
    }

    #[mononoke::test]
    fn test_parse_debugwireargs() {
        let inp = "debugwireargs\n\
                   * 2\n\
                   three 5\nTHREE\
                   empty 0\n\
                   one 3\nONE\
                   two 3\nTWO";
        test_parse(
            inp,
            Request::Single(SingleRequest::Debugwireargs {
                one: b"ONE".to_vec(),
                two: b"TWO".to_vec(),
                all_args: hashmap! {
                    b"one".to_vec() => b"ONE".to_vec(),
                    b"two".to_vec() => b"TWO".to_vec(),
                    b"three".to_vec() => b"THREE".to_vec(),
                    b"empty".to_vec() => vec![],
                },
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_getbundle() {
        // with no arguments
        let inp = "getbundle\n\
                   * 0\n";

        test_parse(
            inp,
            Request::Single(SingleRequest::Getbundle(GetbundleArgs {
                heads: vec![],
                common: vec![],
                bundlecaps: hashset![],
                listkeys: vec![],
                phases: false,
            })),
        );

        // with arguments
        let inp = "getbundle\n\
             * 6\n\
             heads 40\n\
             1111111111111111111111111111111111111111\
             common 81\n\
             2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\
             bundlecaps 14\n\
             cap1,CAP2,cap3\
             listkeys 9\n\
             key1,key2\
             phases 1\n\
             1\
             extra 5\n\
             extra";
        test_parse(
            inp,
            Request::Single(SingleRequest::Getbundle(GetbundleArgs {
                heads: vec![hash_ones()],
                common: vec![hash_twos(), hash_threes()],
                bundlecaps: hashset![b"cap1".to_vec(), b"CAP2".to_vec(), b"cap3".to_vec()],
                listkeys: vec![b"key1".to_vec(), b"key2".to_vec()],
                phases: true,
            })),
        );
    }

    #[mononoke::test]
    fn test_parse_heads() {
        let inp = "heads\n";

        test_parse(inp, Request::Single(SingleRequest::Heads {}));
    }

    #[mononoke::test]
    fn test_parse_hello() {
        let inp = "hello\n";

        test_parse(inp, Request::Single(SingleRequest::Hello {}));
    }

    #[mononoke::test]
    fn test_parse_listkeys() {
        let inp = "listkeys\n\
                   namespace 9\n\
                   bookmarks";

        test_parse(
            inp,
            Request::Single(SingleRequest::Listkeys {
                namespace: "bookmarks".to_string(),
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_lookup() {
        let inp = "lookup\n\
                   key 9\n\
                   bookmarks";

        test_parse(
            inp,
            Request::Single(SingleRequest::Lookup {
                key: "bookmarks".to_string(),
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_lookup2() {
        let inp = "lookup\n\
                   key 4\n\
                   5c79";

        test_parse(
            inp,
            Request::Single(SingleRequest::Lookup {
                key: "5c79".to_string(),
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_gettreepack() {
        let inp = "gettreepack\n\
                   * 4\n\
                   rootdir 0\n\
                   mfnodes 40\n\
                   1111111111111111111111111111111111111111\
                   basemfnodes 40\n\
                   1111111111111111111111111111111111111111\
                   directories 0\n";

        test_parse(
            inp,
            Request::Single(SingleRequest::Gettreepack(GettreepackArgs {
                rootdir: MPath::ROOT,
                mfnodes: vec![hash_ones_manifest()],
                basemfnodes: btreeset![hash_ones_manifest()],
                directories: vec![],
                depth: None,
            })),
        );

        let inp = "gettreepack\n\
             * 5\n\
             depth 1\n\
             1\
             rootdir 5\n\
             ololo\
             mfnodes 81\n\
             1111111111111111111111111111111111111111 2222222222222222222222222222222222222222\
             basemfnodes 81\n\
             2222222222222222222222222222222222222222 1111111111111111111111111111111111111111\
             directories 1\n\
             ,";

        test_parse(
            inp,
            Request::Single(SingleRequest::Gettreepack(GettreepackArgs {
                rootdir: MPath::new("ololo").unwrap(),
                mfnodes: vec![hash_ones_manifest(), hash_twos_manifest()],
                basemfnodes: btreeset![hash_twos_manifest(), hash_ones_manifest()],
                directories: vec![Bytes::from("".as_bytes())],
                depth: Some(1),
            })),
        );

        let inp = "gettreepack\n\
             * 5\n\
             depth 1\n\
             1\
             rootdir 5\n\
             ololo\
             mfnodes 81\n\
             1111111111111111111111111111111111111111 2222222222222222222222222222222222222222\
             basemfnodes 81\n\
             2222222222222222222222222222222222222222 1111111111111111111111111111111111111111\
             directories 6\n\
             :o,:s,";

        test_parse(
            inp,
            Request::Single(SingleRequest::Gettreepack(GettreepackArgs {
                rootdir: MPath::new("ololo").unwrap(),
                mfnodes: vec![hash_ones_manifest(), hash_twos_manifest()],
                basemfnodes: btreeset![hash_twos_manifest(), hash_ones_manifest()],
                directories: vec![Bytes::from(",".as_bytes()), Bytes::from(";".as_bytes())],
                depth: Some(1),
            })),
        );

        let inp = "gettreepack\n\
                   * 4\n\
                   rootdir 0\n\
                   mfnodes 40\n\
                   1111111111111111111111111111111111111111\
                   basemfnodes 40\n\
                   1111111111111111111111111111111111111111\
                   directories 5\n\
                   ,foo,";

        test_parse(
            inp,
            Request::Single(SingleRequest::Gettreepack(GettreepackArgs {
                rootdir: MPath::ROOT,
                mfnodes: vec![hash_ones_manifest()],
                basemfnodes: btreeset![hash_ones_manifest()],
                directories: vec![Bytes::from(b"".as_ref()), Bytes::from(b"foo".as_ref())],
                depth: None,
            })),
        );
    }

    #[mononoke::test]
    fn test_parse_known_1() {
        let inp = "known\n\
                   * 0\n\
                   nodes 40\n\
                   1111111111111111111111111111111111111111";

        test_parse(
            inp,
            Request::Single(SingleRequest::Known {
                nodes: vec![hash_ones()],
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_known_2() {
        let inp = "known\n\
                   * 0\n\
                   nodes 0\n";

        test_parse(inp, Request::Single(SingleRequest::Known { nodes: vec![] }));
    }

    fn test_parse_unbundle_with(bundle: &[u8]) {
        let inp = b"unbundle\n\
                    heads 10\n\
                    666f726365"; // "force" hex encoded

        test_parse_with_extra(
            inp,
            Request::Single(SingleRequest::Unbundle {
                heads: vec![String::from("666f726365")], // "force" in hex-encoding
            }),
            bundle,
        );
    }

    #[mononoke::test]
    fn test_parse_unbundle_minimal() {
        let bundle: &[u8] = &b"HG20\0\0\0\0\0\0\0\0"[..];
        test_parse_unbundle_with(bundle);
    }

    #[mononoke::test]
    fn test_parse_unbundle_small() {
        let bundle: &[u8] = &include_bytes!("../../fixtures/min.bundle")[..];
        test_parse_unbundle_with(bundle);
    }

    #[mononoke::test]
    fn test_batch_parse_heads() {
        match parse_with_params(b"heads\n", batch_params) {
            Ok((rest, val)) => {
                assert!(rest.is_empty());
                assert_eq!(val, SingleRequest::Heads {});
            }
            Err(Err::Incomplete(_)) => panic!("unexpected incomplete input"),
            Err(Err::Error(err) | Err::Failure(err)) => panic!("failed with {:?}", err),
        }
    }

    #[mononoke::test]
    fn test_parse_batch_heads() {
        let inp = "batch\n\
                   * 0\n\
                   cmds 116\n\
                   heads ;\
                   lookup key=1234;\
                   known nodes=1111111111111111111111111111111111111111 \
                   2222222222222222222222222222222222222222";

        test_parse(
            inp,
            Request::Batch(vec![
                SingleRequest::Heads {},
                SingleRequest::Lookup {
                    key: "1234".to_string(),
                },
                SingleRequest::Known {
                    nodes: vec![hash_ones(), hash_twos()],
                },
            ]),
        );
    }

    #[mononoke::test]
    fn test_parse_stream_out_shallow() {
        let inp = "stream_out_shallow\n\
                   * 1\n\
                   noflatmanifest 4\n\
                   True";

        test_parse(
            inp,
            Request::Single(SingleRequest::StreamOutShallow { tag: None }),
        );
    }

    #[mononoke::test]
    fn test_parse_listkeyspatterns() {
        let input = "listkeyspatterns\n\
                     namespace 9\n\
                     bookmarkspatterns 27\n\
                     746573742f2a 6e75636c696465";
        test_parse(
            input,
            Request::Single(SingleRequest::ListKeysPatterns {
                namespace: "bookmarks".to_string(),
                patterns: vec!["test/*".to_string(), "nuclide".to_string()],
            }),
        );
    }

    #[mononoke::test]
    fn test_parse_getcommitdata() {
        let input = "getcommitdata\n\
                     nodes 81\n\
                     1111111111111111111111111111111111111111 2222222222222222222222222222222222222222";
        test_parse(
            input,
            Request::Single(SingleRequest::GetCommitData {
                nodes: vec![hash_ones(), hash_twos()],
            }),
        );
    }
}
