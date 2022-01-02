use futures::TryFutureExt;
use ipfs_api::{request::FilesWrite, IpfsApi};

use crate::error::*;

pub(crate) async fn write_file(
    ipfs: &impl IpfsApi,
    path: String,
    data: Vec<u8>,
) -> Result<(), Error> {
    let opts = FilesWrite {
        path: path.as_str(),
        create: Some(true),
        parents: Some(true),
        cid_version: Some(1),
        ..Default::default()
    };

    tryhard::retry_fn(|| async {
        ipfs.files_write_with_options(opts.clone(), std::io::Cursor::new(data.clone()))
            .map_err(|e| Error::ipfs(format!("{} {:?}", e, opts)))
            .await
    })
    .retries(10)
    .await
}
