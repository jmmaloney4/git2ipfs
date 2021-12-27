use futures::{FutureExt, TryFutureExt};
use ipfs_api::{request::Add, IpfsApi};
use snafu::ResultExt;

use crate::error::*;

pub(crate) async fn add(ipfs: &impl IpfsApi, data: Vec<u8>) -> Result<cid::Cid, Error> {
    let len = data.len();
    let request = ipfs
        .add_with_options(
            std::io::Cursor::new(data),
            Add::builder().cid_version(1).build(),
        )
        .map_err(|e| Error::ipfs(e))
        .map(|res| match res {
            Ok(res) => {
                let size = res.size.parse::<usize>().context(Parse)?;

                if size != len {
                    return Err(Error::MismatchedSizes {
                        data: len,
                        ipfs: size,
                    });
                } else {
                    Ok(res.hash.parse::<cid::Cid>().context(Cid)?)
                }
            }
            Err(e) => Err(e),
        });
    request.await
}
