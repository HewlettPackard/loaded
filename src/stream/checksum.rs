use anyhow::bail;
use async_trait::async_trait;
use futures::Stream;
use futures_util::{future, StreamExt};
use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Checksum {
    Md5,
    Crc32,
    Crc32c,
    Sha1,
    Sha2,
}

impl FromStr for Checksum {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "md5" => Checksum::Md5,
            "crc32" => Checksum::Crc32,
            "crc32c" => Checksum::Crc32c,
            "sha1" => Checksum::Sha1,
            "sha2" => Checksum::Sha2,
            s => {
                bail!("Invalid checksum algorithm '{}'.", s);
            }
        })
    }
}

const CRC32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);

#[async_trait(?Send)]
pub trait StreamedChecksum {
    async fn apply<S: Stream<Item = I>, I: AsRef<[u8]>>(&self, stream: S) -> String;
}

#[async_trait(?Send)]
impl StreamedChecksum for Checksum {
    async fn apply<S: Stream<Item = I>, I: AsRef<[u8]>>(&self, stream: S) -> String {
        match self {
            Checksum::Md5 => {
                let mut hasher = Md5::new();

                let fut = stream.for_each(|f| {
                    hasher.update(&f);
                    future::ready(())
                });
                fut.await;

                format!("{:x}", hasher.finalize())
            }
            Checksum::Crc32 => {
                let mut digest = CRC32.digest();

                let fut = stream.for_each(|f| {
                    digest.update(&f.as_ref());
                    future::ready(())
                });
                fut.await;

                format!("{:x}", digest.finalize())
            }
            Checksum::Crc32c => {
                let mut crc = 0;

                let fut = stream.for_each(|f| {
                    crc = crc32c_hw::update(crc, &f);
                    future::ready(())
                });
                fut.await;

                format!("{crc:x}")
            }
            Checksum::Sha1 => {
                let mut hasher = Sha1::new();

                let fut = stream.for_each(|f| {
                    hasher.update(&f);
                    future::ready(())
                });
                fut.await;

                format!("{:x}", hasher.finalize())
            }
            Checksum::Sha2 => {
                let mut hasher = Sha256::new();

                let fut = stream.for_each(|f| {
                    hasher.update(&f);
                    future::ready(())
                });
                fut.await;

                format!("{:x}", hasher.finalize())
            }
        }
    }
}

#[async_trait(?Send)]
pub trait FullChecksum {
    async fn apply<B: AsRef<[u8]>>(&self, buf: B) -> String;
}

#[async_trait(?Send)]
impl FullChecksum for Checksum {
    async fn apply<B: AsRef<[u8]>>(&self, buf: B) -> String {
        match self {
            Checksum::Md5 => {
                let mut hasher = Md5::new();
                hasher.update(&buf);
                format!("{:x}", hasher.finalize())
            }
            Checksum::Crc32 => {
                let mut digest = CRC32.digest();
                digest.update(&buf.as_ref());
                format!("{:x}", digest.finalize())
            }
            Checksum::Crc32c => {
                let mut crc = 0;
                crc = crc32c_hw::update(crc, &buf);
                format!("{crc:x}")
            }
            Checksum::Sha1 => {
                let mut hasher = Sha1::new();
                hasher.update(&buf);
                format!("{:x}", hasher.finalize())
            }
            Checksum::Sha2 => {
                let mut hasher = Sha256::new();
                hasher.update(&buf);
                format!("{:x}", hasher.finalize())
            }
        }
    }
}
