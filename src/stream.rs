pub mod checksum;
pub mod perpetual_stream;

use crate::stream::checksum::Checksum;
use async_trait::async_trait;
use futures::Stream;

#[async_trait(?Send)]
pub trait StreamProvider<S>: Send
where
    S: Stream,
{
    fn new_stream(&mut self) -> S;
    async fn new_stream_with_checksum(&mut self, checksum: &Checksum) -> (S, String);
    fn empty(&mut self) -> S;
}
