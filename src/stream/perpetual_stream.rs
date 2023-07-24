use crate::stream::checksum::{Checksum, StreamedChecksum};
use crate::stream::StreamProvider;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::Stream;
use futures_util::StreamExt;
use hyper::body::Frame;
use std::cmp::min;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};

#[cfg(target_os = "linux")]
use std::fs;

const CHUNK_SIZE: usize = 16384;

/// A byte stream that operates on an underlying buffer of fixed size.
///
/// It produces `num_to_read` bytes by reading from an inner buffer and wrapping
/// back on itself when it reaches the end.
pub struct PerpetualByteStream {
    inner: Bytes,
    idx: usize,
    num_to_read: usize,
    num_read: usize,
}

impl PerpetualByteStream {
    pub fn new(inner: Bytes, starting_offset: usize, len: usize) -> Self {
        PerpetualByteStream {
            inner,
            idx: starting_offset,
            num_to_read: len,
            num_read: 0,
        }
    }
}

impl Stream for PerpetualByteStream {
    type Item = std::io::Result<Frame<Bytes>>;
    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut pinned = self.as_mut();

        if pinned.num_read < pinned.num_to_read {
            // More to read
            let idx = pinned.idx;
            let num_read = pinned.num_read;
            let num_to_read = pinned.num_to_read;

            let chunk_size = min(num_to_read - num_read, CHUNK_SIZE);
            let as_slice = pinned.inner.as_ref();

            let subslice: Bytes = if idx + chunk_size <= pinned.inner.len() {
                let s = &as_slice[idx..idx + chunk_size];
                let s = pinned.inner.slice_ref(s);
                pinned.idx = idx + chunk_size;
                s
            } else {
                let s = &as_slice[idx..];
                let s = pinned.inner.slice_ref(s);
                pinned.idx = 0;
                s
            };

            self.num_read += subslice.len();

            Poll::Ready(Some(Ok(Frame::data(subslice))))
        } else {
            Poll::Ready(None)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.num_to_read))
    }
}

/// A supplier of `PerpetualByteStreams`
///
/// As `PerpetualByteStream`'s are requested, increments the starting offset by
/// a fixed amount to simulate pseudo-random data. Internally stores a cache
/// of precalculated checksums to not bog down loaded during runtime
pub struct PerpetualByteStreamSupplier {
    buf: Bytes,
    offset: usize,
    len: usize,
    checksum_cache: HashMap<StreamCacheKey, String>,
}

impl PerpetualByteStreamSupplier {
    pub fn new(buf: Bytes, offset: usize, len: usize) -> Self {
        PerpetualByteStreamSupplier {
            buf,
            offset,
            len,
            checksum_cache: HashMap::default(),
        }
    }

    pub async fn with_checksums(
        buf: Bytes,
        offset: usize,
        len: usize,
        checksum: &[Checksum],
    ) -> Self {
        let cache = warm_cache(checksum, &buf, offset, len).await;

        PerpetualByteStreamSupplier {
            buf,
            offset,
            len,
            checksum_cache: cache,
        }
    }
}

async fn warm_cache(
    checksums: &[Checksum],
    buf: &Bytes,
    offset: usize,
    len: usize,
) -> HashMap<StreamCacheKey, String> {
    let mut md5_cache: HashMap<StreamCacheKey, String> = HashMap::default();
    let mut offset = offset;
    for checksum in checksums {
        loop {
            offset = (offset + cache_line_size()) % (buf.len());
            let key = StreamCacheKey {
                checksum: *checksum,
                offset,
                len,
            };
            if let Entry::Vacant(e) = md5_cache.entry(key) {
                let stream = PerpetualByteStream::new(buf.clone(), offset, len);
                e.insert(
                    checksum
                        .apply(stream.map(|i| i.unwrap().into_data().unwrap()))
                        .await,
                );
            } else {
                break;
            }
        }
    }

    md5_cache
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
struct StreamCacheKey {
    checksum: Checksum,
    offset: usize,
    len: usize,
}

// For use when we don't specifically know the size
const DEFAULT_CACHE_LINE_SIZE: usize = 64;

fn cache_line_size() -> usize {
    static MEM: OnceLock<usize> = OnceLock::new();
    *MEM.get_or_init(|| {
        #[cfg(target_os = "linux")]
        {
            fs::read_to_string("/sys/devices/system/cpu/cpu0/cache/index0/coherency_line_size")
                .map_err(Box::<dyn std::error::Error>::from)
                .and_then(|s| s.parse::<usize>().map_err(Into::into))
                .unwrap_or(DEFAULT_CACHE_LINE_SIZE)
        }
        #[cfg(not(target_os = "linux"))]
        DEFAULT_CACHE_LINE_SIZE
    })
}

#[async_trait(?Send)]
impl StreamProvider<PerpetualByteStream> for PerpetualByteStreamSupplier {
    fn new_stream(&mut self) -> PerpetualByteStream {
        let stream = PerpetualByteStream::new(self.buf.clone(), self.offset, self.len);
        self.offset = (self.offset + cache_line_size()) % (self.buf.len());
        stream
    }

    async fn new_stream_with_checksum(
        &mut self,
        checksum: &Checksum,
    ) -> (PerpetualByteStream, String) {
        let key = StreamCacheKey {
            checksum: *checksum,
            offset: self.offset,
            len: self.len,
        };

        let checksum = match self.checksum_cache.entry(key) {
            Entry::Occupied(e) => e.get().clone(),
            Entry::Vacant(v) => {
                let stream = PerpetualByteStream::new(self.buf.clone(), self.offset, self.len);
                let checksum = checksum
                    .apply(stream.map(|i| i.unwrap().into_data().unwrap()))
                    .await;
                v.insert(checksum.clone());
                checksum
            }
        };

        let stream = PerpetualByteStream::new(self.buf.clone(), self.offset, self.len);
        self.offset = (self.offset + cache_line_size()) % (self.buf.len());
        (stream, checksum)
    }
}
