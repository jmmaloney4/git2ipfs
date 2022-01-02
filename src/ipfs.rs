use futures::TryFutureExt;
use ipfs_api::{request::FilesWrite, IpfsApi};

use crate::error::*;

pub(crate) async fn write_file<R>(ipfs: &impl IpfsApi, path: String, data: R) -> Result<()>
where
    R: 'static + std::io::Read + Sync + Send,
{
    let opts = FilesWrite {
        path: path.as_str(),
        create: Some(true),
        parents: Some(true),
        cid_version: Some(1),
        ..Default::default()
    };

    ipfs.files_write_with_options(opts.clone(), data)
        .map_err(|e| Error::ipfs(format!("{} {:?}", e, opts)))
        .await
}
