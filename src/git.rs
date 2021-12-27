use std::pin::Pin;

use futures::{Future, future::BoxFuture};
use git2::{Odb, OdbObject, Oid, References, Repository};
use ipfs_api::IpfsApi;
use itertools::Itertools;

use crate::error::*;
use snafu::ResultExt;

/// Return the object ids for all objects in the object database.
pub(crate) fn oids(odb: &Odb) -> Result<Vec<Oid>, Error> {
    let mut ids = Vec::<Oid>::new();
    odb.foreach(|oid| {
        ids.push(oid.clone());
        true
    })
    .context(Git)?;
    return Ok(ids);
}

pub(crate) async fn save_object(
    ipfs: &impl IpfsApi,
    oid: String,
    data: Vec<u8>,
) -> Result<(String, cid::Cid), Error> {
    let cid = crate::ipfs::add(ipfs, data).await?;
    Ok((
        format!("objects/{}/{}", &oid[..2], &oid[2..]).to_owned(),
        cid,
    ))
}

// pub(crate) fn save_object_futures(
//     ipfs: &impl IpfsApi,
//     odb: Odb,
// ) -> impl Iterator<Item = Pin<Box<dyn Future<Output = Result<(String, cid::Cid), Error>>>>> {
//     oids(&odb).unwrap().into_iter().map(|oid| -> Pin<Box<dyn Future<Output = _>>> {
//         let data = odb.read(oid).context(crate::error::Git).unwrap().data().to_vec();
//         Box::pin(save_object(ipfs, oid.to_string(), data))
//     })
// }

pub(crate) fn generate_info_refs(refs: References) -> Result<String, Error> {
    refs.map(|res| match res {
        Err(e) => Err(Error::Git { source: e }),
        Ok(r) => Ok(r),
    })
    .filter_ok(|r| !r.is_remote())
    .fold(Ok(String::new()), |x, y| -> Result<String, Error> {
        match (x, y) {
            (Err(e), _) | (_, Err(e)) => Err(e),
            (Ok(x), Ok(y)) => {
                let name = match y.name() {
                    None => return Err(RefError::RefHadNoName {}).context(Ref),
                    Some(name) => name,
                };

                let target = match y.target() {
                    None => {
                        return Err(RefError::RefHadNoTarget {
                            name: name.to_string(),
                        })
                        .context(Ref)
                    }
                    Some(target) => target,
                };

                Ok(x + format!("{}\t{}\n", name, target).as_str())
            }
        }
    })
}

pub(crate) async fn save_info_refs<'a>(
    ipfs: &impl IpfsApi,
    refs: References<'a>,
) -> Result<(String, cid::Cid), Error> {
    Result::<_, Error>::Ok((
        "/info/refs".to_owned(),
        crate::ipfs::add(ipfs, generate_info_refs(refs)?.into_bytes()).await?,
    ))
}
