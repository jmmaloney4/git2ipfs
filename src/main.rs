use error::Error;
use futures::{future::BoxFuture, stream, Future, Stream, StreamExt, TryFutureExt, TryStreamExt};
use git::{all_oids, generate_info_refs, generate_ref};
use git2::{Odb, OdbObject, Oid, References, Repository};
use itertools::Itertools;
use snafu::{OptionExt, ResultExt};
use std::{
    iter::{once, once_with},
    pin::Pin,
};

mod error;
mod git;
mod ipfs;

const QUEUE_SIZE: usize = 256;

#[tokio::main]
async fn main() {
    let path = match std::env::args().nth(1) {
        None => panic!("Please supply a path"),
        Some(path) => path,
    };

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let repo = Repository::open(path).context(error::Git).unwrap();
    let odb = repo.odb().context(error::Git).unwrap();

    // println!("{}", repo.head().unwrap().kind().unwrap());

    let objects = all_oids(&odb).unwrap().into_iter().map(|oid| {
        let data = odb.read(oid).context(crate::error::Git)?.data().to_vec();
        let hash = oid.to_string();
        let path = format!("/objects/{}/{}", &hash[..2], &hash[2..]).to_owned();
        Ok((path, data))
    });

    let info_refs = once_with(|| {
        Result::<_, error::Error>::Ok((
            "/info/refs".to_owned(),
            generate_info_refs(repo.references().context(error::Git)?)?.into_bytes(),
        ))
    });

    let head = once_with(|| {
        Result::<_, error::Error>::Ok((
            "/HEAD".to_owned(),
            generate_ref(repo.head().context(error::Git)?)?,
        ))
    })
    .map_ok(|(path, data)| (path, data.into_bytes()));

    let prefix = git::gen_temp_dir_path();
    let mut futures = stream::iter(objects.chain(info_refs).chain(head)).map(|res| async {
        match res {
            Err(e) => Err(e),
            Ok((path, data)) => ipfs::write_file(&ipfs, format!("/{}/{}", prefix, path), data).await,
        }
    }).buffer_unordered(QUEUE_SIZE);    

    if let Err(e) = async {
        while let Some(x) = futures.next().await {
            match x {
                Err(e) => return Err(e),
                Ok(_) => continue,
            }
        }
        Ok(())
    }
    .await
    {
        panic!("{}", e);
    }

    match async {
        ipfs_api::IpfsApi::files_stat(&ipfs, format!("/{}", prefix).as_str()).await
    }.await {
        Err(e) => panic!("{}", e),
        Ok(res) => {
            println!("{}", res.hash);
        }
    }
}
