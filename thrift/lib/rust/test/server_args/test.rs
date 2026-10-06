/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::ffi::CStr;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use fbthrift::ApplicationException;
use fbthrift::BinaryProtocol;
use fbthrift::DummyRequestContext;
use fbthrift::FramingDecoded;
use fbthrift::FramingEncodedFinal;
use fbthrift::MessageType;
use fbthrift::Protocol;
use fbthrift::ProtocolID;
use fbthrift::ProtocolWriter;
use fbthrift::ReplyState;
use fbthrift::Serialize;
use fbthrift::SerializedStreamElement;
use fbthrift::ThriftService;
use fbthrift::builtin_types::Bytes;
use futures::executor::block_on;
use futures::stream;
use futures::stream::BoxStream;
use thrift_test::AdaptedInner;
use thrift_test::Choice;
use thrift_test::Color;
use thrift_test::Inner;
use thrift_test::NonZeroId;
use thrift_test::PartialArgs;
use thrift_test::WrappedAdaptedInner;
use thrift_test_services::ServerArgs;
use thrift_test_services::ServerArgsProcessor;
use thrift_test_services::errors::server_args::EchoExn;
use thrift_test_services::errors::server_args::NonZeroExn;
use thrift_test_services::errors::server_args::WrappedExn;

type RequestContext = DummyRequestContext<CStr, <BinaryProtocol as Protocol>::Frame>;

struct DiscardReplies;

impl ReplyState<Bytes> for DiscardReplies {
    type RequestContext = RequestContext;

    fn send_reply(&self, _reply: FramingEncodedFinal<Bytes>) {}

    fn send_stream_reply(
        &self,
        _response: FramingEncodedFinal<Bytes>,
        _stream: Option<BoxStream<'static, SerializedStreamElement<FramingEncodedFinal<Bytes>>>>,
        _protocol_id: ProtocolID,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn send_sink_reply(
        &self,
        _first_response: FramingEncodedFinal<Bytes>,
        _buffer_size: u64,
        _chunk_timeout: std::time::Duration,
        _protocol_id: ProtocolID,
    ) -> (
        BoxStream<'static, Result<FramingDecoded<Bytes>, ApplicationException>>,
        impl FnOnce(SerializedStreamElement<FramingEncodedFinal<Bytes>>) + Send,
    ) {
        (Box::pin(stream::empty()), |_| {})
    }

    fn send_bidirectional_reply(
        &self,
        _first_response: FramingEncodedFinal<Bytes>,
        _stream: BoxStream<'static, SerializedStreamElement<FramingEncodedFinal<Bytes>>>,
        _protocol_id: ProtocolID,
    ) -> BoxStream<'static, Result<FramingDecoded<Bytes>, ApplicationException>> {
        Box::pin(stream::empty())
    }
}

#[derive(Debug, PartialEq)]
struct Received {
    number: i32,
    text: String,
    numbers: Vec<i64>,
    inner: Inner,
    color: Color,
    choice: Choice,
    adapted: AdaptedInner,
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Option<Received>>>);

#[async_trait]
impl ServerArgs for Recorder {
    async fn echo(
        &self,
        number: i32,
        text: String,
        numbers: Vec<i64>,
        inner: Inner,
        color: Color,
        choice: Choice,
        adapted: AdaptedInner,
    ) -> Result<(), EchoExn> {
        *self.0.lock().unwrap() = Some(Received {
            number,
            text,
            numbers,
            inner,
            color,
            choice,
            adapted,
        });
        Ok(())
    }

    async fn non_zero(&self, _id: NonZeroId) -> Result<(), NonZeroExn> {
        Ok(())
    }

    async fn wrapped(&self, _wrapped: WrappedAdaptedInner) -> Result<(), WrappedExn> {
        Ok(())
    }
}

/// Returns what `echo` received, or `None` for the other methods.
fn call(method: &str, args: &PartialArgs) -> anyhow::Result<Option<Received>> {
    let recorder = Recorder::default();
    let processor = ServerArgsProcessor::<BinaryProtocol, _, RequestContext, DiscardReplies>::new(
        recorder.clone(),
    );
    let request = fbthrift::serialize!(BinaryProtocol, |p| {
        p.write_message_begin(method, MessageType::Call, 0);
        args.rs_thrift_write(p);
        p.write_message_end();
    });
    block_on(processor.call(
        Cursor::new(request),
        &RequestContext::new(),
        Arc::new(DiscardReplies),
    ))?;
    Ok(recorder.0.lock().unwrap().take())
}

fn default_inner() -> Inner {
    Inner {
        plain: 0,
        with_default: 7,
        ..Default::default()
    }
}

fn intrinsic_defaults() -> Received {
    Received {
        number: 0,
        text: String::new(),
        numbers: Vec::new(),
        inner: default_inner(),
        color: Color(0),
        choice: Choice::default(),
        adapted: default_inner(),
    }
}

#[test]
fn absent_args_take_intrinsic_defaults() -> anyhow::Result<()> {
    assert_eq!(
        call("echo", &PartialArgs::default())?,
        Some(intrinsic_defaults())
    );
    Ok(())
}

#[test]
fn present_args_are_decoded() -> anyhow::Result<()> {
    let args = PartialArgs {
        number: Some(5),
        text: Some("hello".to_owned()),
        adapted: Some(Inner {
            plain: 3,
            ..default_inner()
        }),
        ..Default::default()
    };
    let expected = Received {
        number: 5,
        text: "hello".to_owned(),
        adapted: Inner {
            plain: 3,
            ..default_inner()
        },
        ..intrinsic_defaults()
    };
    assert_eq!(call("echo", &args)?, Some(expected));
    Ok(())
}

#[test]
fn absent_arg_whose_adapter_rejects_the_default_is_an_error() {
    let err = call("non_zero", &PartialArgs::default()).unwrap_err();
    assert!(
        format!("{err:#}").contains("`ServerArgs.non_zero` missing arg `id`"),
        "{err:#}"
    );

    let present = PartialArgs {
        id: Some(3),
        ..Default::default()
    };
    assert!(call("non_zero", &present).is_ok());
}

#[test]
fn absent_newtype_over_adapted_arg_is_an_error() {
    let err = call("wrapped", &PartialArgs::default()).unwrap_err();
    assert!(
        format!("{err:#}").contains("`ServerArgs.wrapped` missing arg `wrapped`"),
        "{err:#}"
    );

    let present = PartialArgs {
        wrapped: Some(default_inner()),
        ..Default::default()
    };
    assert!(call("wrapped", &present).is_ok());
}
