// use std::{
//     collections::HashMap,
//     fmt::{format, Display},
//     pin::Pin,
// };

// use futures::{
//     channel::mpsc,
//     stream::{BufferUnordered, FuturesUnordered},
//     Future, FutureExt, SinkExt, Stream, StreamExt, TryFutureExt, TryStreamExt,
// };
// use hyper::client::HttpConnector;
// use indicatif::ProgressBar;
// use ipfs_api::{request::Add, response::AddResponse, IpfsApi, IpfsClient};
// use itertools::Itertools;

// #[tokio::main]
// async fn main() {
//     match _main().await {
//         Err(e) => panic!("{}", e),
//         _ => (),
//     }
// }

// async fn _main() -> Result<(), Error> {
//     let path = match std::env::args().nth(1) {
//         None => panic!("Please supply a path"),
//         Some(path) => path,
//     };

//     let repo = Repository::open(path).context(Git)?;

//     match generate_info_refs(repo.references().context(Git)?) {
//         Err(e) => return Err(e),
//         Ok(s) => println!("{}", s),
//     }

//     repo.references()
//         .context(Git)?
//         .collect::<Result<Vec<_>, _>>()
//         .context(Git)?
//         .into_iter()
//         .filter(|r| !r.is_remote())
//         .for_each(|r| {
//             println!(
//                 "{:?} {} {}",
//                 r.name().unwrap(),
//                 r.kind().unwrap(),
//                 r.target().unwrap()
//             )
//         });

//     let odb = repo.odb().context(Git)?;
//     let ids = collect_git_oids(&odb)?;
//     let progress = ProgressBar::new(ids.len().try_into().unwrap_or(0));

//     let ipfs = IpfsClient::<HttpConnector>::default();
//     let futures = ids
//         .into_iter()
//         .map(|oid| Ok(add_git_object(&ipfs, odb.read(oid).context(Git)?)))
//         .collect::<Result<Vec<_>, Error>>()?;

//     let mut objects = futures::stream::iter(futures).buffer_unordered(QUEUE_SIZE);

//     while let Some(res) = objects.next().await {
//         match res {
//             Err(e) => {
//                 // println!("Error!: {}", e);
//                 progress.abandon_with_message(format!("{}", e));
//                 return Err(e);
//             }
//             Ok((cid, oid, size)) => {
//                 progress.inc(1);
//                 progress.println(format!("{} - {} ({})", oid, cid.to_string(), size))
//             }
//         }
//     }

//     progress.finish();
//     println!("Finished in {:?}", progress.elapsed());

//     Ok(())
// }

// use rand::distributions::{Alphanumeric, DistString};
// use rand::thread_rng;
// fn gen_temp_dir_path() -> String {
//     const TMP_PATH_LEN: usize = 19;
//     Alphanumeric.sample_string(&mut thread_rng(), TMP_PATH_LEN)
// }

// /// Return the object ids for all objects in the object database.
// fn collect_git_oids(odb: &Odb) -> Result<Vec<Oid>, Error> {
//     let mut ids = Vec::<Oid>::new();
//     odb.foreach(|oid| {
//         ids.push(oid.clone());
//         true
//     })
//     .context(Git)?;
//     return Ok(ids);
// }

// async fn add_git_object(
//     ipfs: &impl IpfsApi,
//     obj: OdbObject<'_>,
// ) -> Result<(cid::Cid, Oid, usize), Error> {
//     println!("{}", obj.kind());
//     let (cid, size) = add(ipfs, obj.data().to_vec()).await?;

//     Ok((cid, obj.id(), size))
// }

// async fn add_file(ipfs: &impl IpfsApi, object: Cid, path: String) {
//     // ipfs.files_write(path, create, truncate, data)
// }

// use async_stream::try_stream;
// use futures::channel::mpsc::channel;

// // fn git_repo_stream(repo: Repository, ipfs: &impl IpfsApi) -> impl Stream<Item = Result<(String, Cid), Error>> {
// //     try_stream! {
// //         yield ;
// //     }
// // }

// async fn write_ipfs_file(ipfs: &impl IpfsApi, path: String, cid: cid::Cid) -> Result<(), Error> {
//     ipfs.files_cp(path.as_str(), format!("/ipfs/{}", cid).as_str())
//         .map_err(|e| Error::ipfs(e))
//         .await
// }

// async fn complete_future_and_write_ipfs_file(
//     ipfs: &impl IpfsApi,
//     fut: Pin<Box<dyn Future<Output = Result<(String, cid::Cid), Error>>>>,
// ) -> Result<(), Error> {
//     let (path, cid) = match fut.await {
//         Err(e) => return Err(e),
//         Ok(x) => x,
//     };

//     write_ipfs_file(ipfs, path, cid).await
// }

// async fn add_git_repository_to_ipfs(
//     repo: Repository,
//     ipfs: &impl IpfsApi,
// ) -> Result<(), Error> {
//     const QUEUE_SIZE: usize = 256;
//     let (mut tx, rx) =
//         mpsc::unbounded::<Pin<Box<dyn Future<Output = Result<(String, cid::Cid), Error>>>>>();

//     let futures = rx
//         .map(|fut| complete_future_and_write_ipfs_file(ipfs, fut))
//         .buffer_unordered(QUEUE_SIZE);
//     {
//         let odb = repo.odb().context(Git)?;
//         for oid in collect_git_oids(&odb)? {
//             let obj = odb.read(oid).context(Git)?;
//             tx.send(Box::pin(async move {
//                 let res = ipfs.add(obj.data()).map_err(|e| Error::ipfs(e)).await?;
//                 let cid = res.hash.parse::<cid::Cid>().context(Cid)?;
//                 let bytes = oid.to_string().into_bytes();
//                 let prefix = String::from_utf8_lossy(&bytes[0..2]);
//                 let suffix = String::from_utf8_lossy(&bytes[3..]);

//                 Ok((
//                     format!("/objects/{}/{}", prefix, suffix),
//                     cid
//                 ))
//             }));
//         }
//     }

//     tx.send(Box::pin(async move {
//         Ok((
//             "/info/refs".to_owned(),
//             add(
//                 ipfs,
//                 generate_info_refs(repo.references().context(Git)?)?.into_bytes(),
//             )
//             .await?
//             .0,
//         ))
//     }));

//     // let futures =

//     //     .collect::<Result<Vec<_>, Error>>()?;

//     // let iter: Vec<&dyn >

//     // let iter = vec![async {
//     //     Result::<(String, cid::Cid), Error>::Ok((
//     //         "/info/refs".to_owned(),
//     //         add(
//     //             ipfs,
//     //             generate_info_refs(repo.references().context(Git)?)?.into_bytes(),
//     //         )
//     //         .await?
//     //         .0,
//     //     ))
//     // })]
//     // .into_iter()
//     // .chain(ids.into_iter().map(|oid| async {
//     //     match odb.read(oid).context(Git) {
//     //         Err(e) => Err(e),
//     //         Ok(obj) => {
//     //             match add_git_object(ipfs, obj).await {
//     //                 Err(e) => Err(e),
//     //                 Ok((cid, oid, _)) => {
//     //                     let a = oid.to_string().chars();
//     //                     Ok((format!("objects/{}/{}", a.take(2).join(""), a.join("")).to_owned(), cid))
//     //                 }
//     //             }
//     //         }
//     //     };

//     // }));

//     // let (tx, rx) = channel::<Result<(String, cid::Cid), Error>>(QUEUE_SIZE);

//     // tokio::spawn(async {
//     //     tx.send(async { ) }.await);
//     // });

//     // rx.map(|res| {
//     //     async move {
//     //         match res {
//     //             Err(e) => Err(e),
//     //             Ok((path, cid)) => ipfs.files_cp(format!("/ipfs/{}", cid).as_str(), path.as_str()).map_err(|e| Error::ipfs(e)).await
//     //         }
//     //     }
//     // }).buffer_unordered(QUEUE_SIZE);

//     // Ok(("/info/refs".to_owned(), add(ipfs, generate_info_refs(repo.references().context(Git)?)?.into_bytes()).await?.0));

//     // let odb = repo.odb().context(Git)?;
//     // let ids = collect_git_oids(&odb)?;

//     // let futures = ids.into_iter().map(|oid| Ok(add_git_object(&ipfs, odb.read(oid).context(Git)?))).collect::<Result<Vec<_>, Error>>()?;

//     // let mut objects = futures::stream::iter(futures);

//     Ok(())
// }

use error::Error;
use futures::{future::BoxFuture, stream, Future, Stream, StreamExt, TryFutureExt};
use git::{generate_info_refs, oids};
use git2::{Odb, OdbObject, Oid, References, Repository};
use ipfs_api::IpfsApi;
use itertools::Itertools;
use snafu::ResultExt;
use std::{
    iter::{self, once, Chain},
    pin::Pin,
    rc::Rc,
};

mod error;
mod git;
mod ipfs;

const QUEUE_SIZE: usize = 256;

#[tokio::main]
async fn main() {
    // let s1 = stream::iter(
    //     vec![async {
    //         "Hello, World".to_owned();
    //     }]
    //     .into_iter(),
    // );

    // let s2 = stream::iter(
    //     vec![async {
    //         "FooBar".to_owned();
    //     }]
    //     .into_iter(),
    // );

    // let s3 = s1.chain(s2);

    let path = match std::env::args().nth(1) {
        None => panic!("Please supply a path"),
        Some(path) => path,
    };

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let repo = Repository::open(path).context(error::Git).unwrap();
    let odb = repo.odb().context(error::Git).unwrap();

    println!("{:?}", repo.head().unwrap().name().unwrap());

    let mut futures = stream::iter(
        oids(&odb)
            .unwrap()
            .into_iter()
            .map(|oid| -> Pin<Box<dyn Future<Output = _>>> {
                let data = odb
                    .read(oid)
                    .context(crate::error::Git)
                    .unwrap()
                    .data()
                    .to_vec();
                Box::pin(git::save_object(&ipfs, oid.to_string(), data))
            })
            .chain(once(Box::pin(async {
                Result::<_, error::Error>::Ok((
                    "/info/refs".to_owned(),
                    ipfs::add(
                        &ipfs,
                        generate_info_refs(repo.references().context(error::Git).unwrap())
                            .unwrap()
                            .into_bytes(),
                    )
                    .await
                    .unwrap(),
                ))
            }) as Pin<Box<dyn Future<Output = _>>>)),
    )
    .buffer_unordered(QUEUE_SIZE);

    if let Err(e) = async {
        while let Some(x) = futures.next().await {
            match x {
                Err(e) => return Err(e),
                _ => continue,
            }
        }
        Ok(())
    }
    .await
    {
        panic!("{}", e);
    }

    // let s2 = stream::iter(iter::once(Box::pin(async {
    //     Result::<_, error::Error>::Ok((
    //         "/info/refs",
    //         ipfs::add(
    //             ipfs.as_ref(),
    //             generate_info_refs(repo.references().context(error::Git).unwrap())
    //                 .unwrap()
    //                 .into_bytes(),
    //         )
    //         .await
    //         .unwrap(),
    //     ))
    // })));

    // let s3 = s2.chain(s1);

    // .map(|f| Into::<Pin<Box<dyn Future<Output = Result<(String, cid::Cid), error::Error>>>>>::into(Box::pin(f)))
    // .chain(iter::once(Box::pin(async {
    //     Ok((
    //         "/info/refs",
    //         ipfs::add(
    //             &ipfs,
    //             generate_info_refs(repo.references().context(error::Git).unwrap())
    //                 .unwrap()
    //                 .into_bytes(),
    //         )
    //         .await
    //         .unwrap(),
    //     ))
    // }).into()));
}
