use std::{collections::HashMap, fmt::format};

use futures::{
    stream::{BufferUnordered, FuturesUnordered},
    FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use git2::{Odb, OdbObject, Oid, Repository, References};
use hyper::client::HttpConnector;
use indicatif::ProgressBar;
use ipfs_api::{request::Add, response::AddResponse, IpfsApi, IpfsClient};
use itertools::Itertools;

use snafu::{ResultExt, Snafu, ensure};

const QUEUE_SIZE: usize = 256;

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

    match generate_info_refs(repo.references().context(Git)?) {
        Err(e) => return Err(e),
        Ok(s) => println!("{}", s)
    }

    repo.references()
        .context(Git)?
        .collect::<Result<Vec<_>, _>>()
        .context(Git)?
        .into_iter()
        .filter(|r| !r.is_remote())
        .for_each(|r| {
            println!(
                "{:?} {} {}",
                r.name().unwrap(),
                r.kind().unwrap(),
                r.target().unwrap()
            )
        });

    let odb = repo.odb().context(Git)?;
    let ids = collect_git_oids(&odb)?;
    let progress = ProgressBar::new(ids.len().try_into().unwrap_or(0));

    let ipfs = IpfsClient::<HttpConnector>::default();
    let futures = ids
        .into_iter()
        .map(|oid| Ok(add_git_object(&ipfs, odb.read(oid).context(Git)?)))
        .collect::<Result<Vec<_>, Error>>()?;

    let mut objects = futures::stream::iter(futures).buffer_unordered(QUEUE_SIZE);

    while let Some(res) = objects.next().await {
        match res {
            Err(e) => {
                // println!("Error!: {}", e);
                progress.abandon_with_message(format!("{}", e));
                return Err(e);
            }
            Ok((cid, oid, size)) => {
                progress.inc(1);
                progress.println(format!("{} - {} ({})", oid, cid.to_string(), size))
            }
        }
    }

    progress.finish();
    println!("Finished in {:?}", progress.elapsed());

    Ok(())
}

use rand::distributions::{Alphanumeric, DistString};
use rand::thread_rng;
fn gen_temp_dir_path() -> String {
    const TMP_PATH_LEN: usize = 19;
    Alphanumeric.sample_string(&mut thread_rng(), TMP_PATH_LEN)
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
    ipfs: &impl IpfsApi,
    obj: OdbObject<'_>,
) -> Result<(cid::Cid, Oid, usize), Error> {
    println!("{}", obj.kind());
    let (cid, size) = add(ipfs, obj.data().to_vec()).await?;

    Ok((cid, obj.id(), size))
}

async fn add_file(ipfs: &impl IpfsApi, object: Cid, path: String) {
    // ipfs.files_write(path, create, truncate, data)
}

async fn add(ipfs: &impl IpfsApi, data: Vec<u8>) -> Result<(cid::Cid, usize), Error> {
    let len = data.len();
    let request = ipfs
        .add_with_options(
            std::io::Cursor::new(data),
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

                if size != len {
                    return Err(Error::MismatchedSizes {
                        data: len,
                        ipfs: size,
                    });
                } else {
                    let cid = res.hash.parse::<cid::Cid>().context(Cid)?;
                    Ok((cid, size))
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

    #[snafu(display("Mismatched Sizes: {} (data) ≠ {} (ipfs)", data, ipfs))]
    MismatchedSizes { data: usize, ipfs: usize },

    #[snafu(display("Cid Error: {}", source))]
    Cid { source: cid::Error },

    #[snafu(display("Ref Error: {}", source))]
    Ref { source: RefError },
}

async fn add_git_repository_to_ipfs(repo: Repository, ipfs: &impl IpfsApi) -> Result<(), Error> {
    let mut files = HashMap::<String, Cid>::new();

    let odb = repo.odb().context(Git)?;
    let ids = collect_git_oids(&odb)?;



    Ok(())
}

#[derive(Debug, Snafu)]
enum RefError {
    #[snafu()]
    RefHadNoName {},

    #[snafu()]
    RefHadNoTarget {
        name: String,
    },
}

fn generate_info_refs(refs: References) -> Result<String, Error> {
    refs
    .map(|res| match res {
        Err(e) => Err(Error::Git{ source: e }),
        Ok(r) => Ok(r)
    })
    .filter_ok(|r| !r.is_remote())
    .fold(Ok(String::new()), |x, y| -> Result<String, Error> {
        match (x, y) {
            (Err(e), _) | (_, Err(e)) => Err(e),
            (Ok(x), Ok(y)) => {
                let name = match y.name() {
                    None => return Err(RefError::RefHadNoName {}).context(Ref),
                    Some(name) => name
                };

                let target = match y.target() {
                    None => return Err(RefError::RefHadNoTarget { name: name.to_string() }).context(Ref),
                    Some(target) => target
                };

                Ok(x + format!("{}\t{}\n", name, target).as_str())
            }
        }
    })
}