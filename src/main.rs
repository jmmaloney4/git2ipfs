use std::{fmt::format, process::exit};

use futures::{stream::FuturesUnordered, FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use git2::{Odb, OdbObject, Oid, Repository};
use hyper::client::HttpConnector;
use indicatif::ProgressBar;
use ipfs_api::{request::Add, response::AddResponse, IpfsApi, IpfsClient};
use itertools::Itertools;

use snafu::{ResultExt, Snafu};

#[tokio::main]
async fn main() {
    match _main().await {
        Err(e) => panic!("{}", e),
        _ => (),
    }
}
async fn _main() -> Result<(), Error> {
    let path = match std::env::args().nth(1) {
        None => panic!("Please supply a path"),
        Some(path) => path,
    };

    let repo = Repository::open(path).context(Git)?;
    let odb = repo.odb().context(Git)?;
    let ids = collect_git_oids(&odb)?;
    let progress = ProgressBar::new(ids.len().try_into().unwrap_or(0));

    let client = IpfsClient::<HttpConnector>::default();
    let mut objects = match ids
        .into_iter()
        .map(|oid| {
            let obj = odb.read(oid).context(Git)?;
            Ok(add_git_object(&client, obj))
        })
        .collect::<Result<FuturesUnordered<_>, Error>>()
    {
        Err(e) => panic!("{}", e),
        Ok(objects) => objects,
    };

    while let Some(res) = objects.next().await {
        match res {
            Err(e) => {
                progress.abandon_with_message(format!("{}", e));
                exit(1);
            }
            Ok((cid, oid, size)) => {
                progress.inc(1);
                progress.println(format!("{} - {} ({})", oid, cid.to_string(), size))
            }
        }
    }

    Ok(())
}

/// Return the object ids for all objects in the object database.
fn collect_git_oids(odb: &Odb) -> Result<Vec<Oid>, Error> {
    let mut ids = Vec::<Oid>::new();
    odb.foreach(|oid| {
        ids.push(oid.clone());
        true
    })
    .context(Git)?;
    return Ok(ids);
}

async fn add_git_object(
    client: &impl IpfsApi,
    obj: OdbObject<'_>,
) -> Result<(cid::Cid, Oid, usize), Error> {
    let request = client
        .add_with_options(
            std::io::Cursor::new(obj.data().to_vec()),
            Add::builder().cid_version(1).build(),
        )
        .map_err(|e| Error::Ipfs {
            text: format!("{}", e),
        })
        .map(|res| match res {
            Ok(res) => {
                let size = match res.size.parse::<usize>() {
                    Err(e) => return Err(Error::Parse { source: e }),
                    Ok(size) => size,
                };

                if size != obj.len() {
                    return Err(Error::MismatchedSizes {
                        git: obj.len(),
                        ipfs: size,
                    });
                } else {
                    let cid = res.hash.parse::<cid::Cid>().context(Cid)?;
                    Ok((cid, obj.id(), size))
                }
            }
            Err(e) => Err(e),
        });
    request.await
}

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("Ipfs API Error: {}", text))]
    Ipfs { text: String },

    #[snafu(display("Git Error: {}", source))]
    Git { source: git2::Error },

    #[snafu(display("Parse Int Error {:?}", source))]
    Parse { source: core::num::ParseIntError },

    #[snafu(display("Mismatched Sizes: {} (git) ≠ {} (ipfs)", git, ipfs))]
    MismatchedSizes { git: usize, ipfs: usize },

    #[snafu(display("Cid Error: {}", source))]
    Cid { source: cid::Error },
}
