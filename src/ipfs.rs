use flate2::read::ZlibEncoder;
use futures::TryFutureExt;
use ipfs_api::{request::FilesWrite, IpfsApi};

use crate::error::*;

pub(crate) async fn write_file(
    ipfs: &impl IpfsApi,
    path: String,
    data: Vec<u8>,
) -> Result<(), Error> {
    ipfs.files_write_with_options(
        FilesWrite {
            path: path.as_str(),
            create: Some(true),
            parents: Some(true),
            cid_version: Some(1),
            ..Default::default()
        },
        std::io::Cursor::new(data),
    )
    .map_err(Error::ipfs)
    .await
}
