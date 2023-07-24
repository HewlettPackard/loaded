use crate::engine::Engine;
use crate::stream::checksum::Checksum;
use crate::stream::StreamProvider;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Buf;
use futures::Stream;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::http::request;
use hyper::{Request, Response, Uri};

use std::cell::RefCell;
use std::fmt::Write;
use std::marker::PhantomData;

/// An S3 engine to generate http traffic to an S3 server. This workload
/// will consist of PUTs and GETs to the server.
///
/// This S3 engine does not make use of any provided S3 client but instead manually
/// crafts the requests to ensure control over the payload and push as much load
/// as possible.
pub struct S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream,
{
    stream_supplier: RefCell<P>,
    uri_supplier: RefCell<UriProvider>,
    payload_len: usize,
    phantom: PhantomData<S>,
    checksum_algo: Option<Checksum>,
}

impl<P, S> S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream,
{
    pub fn new(
        stream_supplier: P,
        uri_supplier: UriProvider,
        payload_len: usize,
        checksum_algo: Option<Checksum>,
    ) -> Self {
        S3Engine {
            stream_supplier: RefCell::new(stream_supplier),
            uri_supplier: RefCell::new(uri_supplier),
            payload_len,
            phantom: PhantomData,
            checksum_algo,
        }
    }
}

#[async_trait(? Send)]
impl<P, S, D, E> Engine<StreamBody<S>> for S3Engine<P, S>
where
    P: StreamProvider<S>,
    S: Stream<Item = Result<Frame<D>, E>>,
    D: Buf,
{
    fn name<'a>(&self) -> &'a str {
        "s3"
    }

    async fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    #[allow(clippy::await_holding_refcell_ref)]
    async fn request(&mut self, req: request::Builder) -> Result<(Request<StreamBody<S>>, usize)> {
        let (req, stream) = match &self.checksum_algo {
            None => (req, self.stream_supplier.borrow_mut().new_stream()),
            Some(c) => {
                let (stream, digest) = self
                    .stream_supplier
                    .borrow_mut()
                    .new_stream_with_checksum(c)
                    .await;

                let req = match c {
                    Checksum::Md5 => req.header("Content-MD5", digest),
                    Checksum::Crc32 => req.header("x-amz-checksum-crc32", digest),
                    Checksum::Crc32c => req.header("x-amz-checksum-crc32c", digest),
                    Checksum::Sha1 => req.header("x-amz-checksum-sha1", digest),
                    Checksum::Sha2 => req.header("x-amz-checksum-sha256", digest),
                };
                (req, stream)
            }
        };

        let req = req
            .uri(&self.uri_supplier.borrow_mut().next())
            .method("PUT")
            .header(hyper::header::CONTENT_TYPE, "application/octet-stream")
            .body(StreamBody::new(stream))?;

        Ok((req, self.payload_len))
    }

    async fn response(&mut self, resp: &mut Response<Incoming>) -> Result<usize> {
        let mut read = 0;
        while let Some(next) = resp.frame().await {
            let frame = next.unwrap();
            if let Some(d) = frame.data_ref() {
                read += d.len();
            }
        }
        Ok(read)
    }

    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}

struct ArbitraryRadixNumber {
    digits: Vec<usize>,
    radix: usize,
}

impl ArbitraryRadixNumber {
    fn new(num_digits: usize, radix: usize) -> Self {
        ArbitraryRadixNumber {
            digits: vec![0; num_digits],
            radix,
        }
    }

    fn increment(&mut self) {
        let mut i = self.digits.len() - 1;
        while i < self.digits.len() {
            //increment current digit
            self.digits[i] = (self.digits[i] + 1) % self.radix;
            if self.digits[i] == 0 && i != 0 {
                // we wrapped the current digit, move up a digit
                i -= 1;
            } else {
                break;
            }
        }
    }

    fn to_digits(&self) -> Vec<usize> {
        self.digits.clone()
    }
}

/// The URI provider allows crafting URIs with a specified folder depth to
/// allow stressing s3 implementations that have a performance cost for
/// directories.
pub struct UriProvider {
    base: String,
    bucket: String,
    obj_prefix: String,
    num_objs_per_prefix: usize,
    obj_cnt: usize,
    /// A number to let us build out an incrementing dir prefix where each digit is folder.
    radix_num: Option<ArbitraryRadixNumber>,
}

impl UriProvider {
    pub fn new(
        uri_base: String,
        bucket: String,
        obj_prefix: String,
        depth: usize,
        num_objs: usize,
        num_branch_per_depth: usize,
    ) -> Self {
        let radix_num = if depth > 0 {
            Some(ArbitraryRadixNumber::new(depth, num_branch_per_depth))
        } else {
            None
        };
        UriProvider {
            base: uri_base,
            bucket,
            obj_prefix,
            num_objs_per_prefix: num_objs,
            obj_cnt: 0,
            radix_num,
        }
    }

    pub fn next(&mut self) -> Uri {
        // Build the directory prefix according to the current radix number
        // For instance, if we had the radix_num `321`, that would result in the
        // directory prefix of "3/2/1/"
        let dir_prefix = self.radix_num.as_mut().map_or(String::new(), |n| {
            let mut s = String::new();
            n.to_digits()
                .iter()
                .try_for_each(|i| write!(s, "{i}/"))
                .unwrap();
            s
        });

        // Build the uri, leaving the object prefix at the very top to ensure that all
        // our folders are unique for the run
        let uri = format!(
            "{}/{}/{}{}{}",
            self.base, self.bucket, dir_prefix, self.obj_prefix, self.obj_cnt
        )
        .parse::<Uri>()
        .unwrap();

        self.obj_cnt = (self.obj_cnt + 1) % self.num_objs_per_prefix;

        if self.obj_cnt == 0 {
            // we've written num_objs to the current prefix, increment to get to the next dir prefix.
            if let Some(n) = self.radix_num.as_mut() {
                n.increment();
            }
        }

        uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn no_depth_single_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            0,
            1,
            0,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude0").unwrap(),
        ];

        let actual: Vec<Uri> = (0..3).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn no_depth_multi_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            0,
            2,
            0,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/my-dude0").unwrap(),
        ];

        let actual: Vec<Uri> = (0..3).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn one_depth_single_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            1,
            1,
            1,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude0").unwrap(),
        ];

        let actual: Vec<Uri> = (0..3).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn one_depth_multi_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            1,
            2,
            1,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/my-dude0").unwrap(),
        ];

        let actual: Vec<Uri> = (0..3).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn multi_depth_single_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            2,
            1,
            2,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/1/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/1/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude0").unwrap(),
        ];

        let actual: Vec<Uri> = (0..5).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn multi_depth_multi_obj() {
        let mut s = UriProvider::new(
            "http://10.0.1.24:9003".to_string(),
            "bucket".to_string(),
            "my-dude".to_string(),
            2,
            2,
            2,
        );

        let expected = vec![
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/1/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/1/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/0/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/1/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/1/1/my-dude1").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude0").unwrap(),
            Uri::from_str("http://10.0.1.24:9003/bucket/0/0/my-dude1").unwrap(),
        ];

        let actual: Vec<Uri> = (0..10).map(|_| s.next()).collect();
        assert_eq!(expected, actual);
    }
}
