use clap::{crate_authors, crate_description, crate_name, crate_version, App, Arg};
use flate2::read::ZlibEncoder;
use futures::{stream, StreamExt, TryFutureExt};
use git::{all_oids, generate_info_refs, generate_ref};
use git2::{ObjectType, Repository};
use indicatif::ProgressBar;
use ipfs_api::IpfsApi;
use itertools::Itertools;
use snafu::ResultExt;
use std::{io::Read, iter::once_with, path::PathBuf, process::exit};

mod error;
mod git;
mod ipfs;

const QUEUE_SIZE: usize = 256;

#[tokio::main]
async fn main() {
    const PATH_ARG: &str = "path";
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::with_name(PATH_ARG)
                .required(false)
                .help("Path to the git repository to upload")
                .index(1),
        )
        .get_matches();

    let path = match matches.value_of(PATH_ARG) {
        None => std::env::current_dir().unwrap(),
        Some(path) => PathBuf::from(path),
    };

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let repo = Repository::open(path).context(error::Git).unwrap();
    let odb = repo.odb().context(error::Git).unwrap();
    let oids = all_oids(&odb).unwrap();
    let n = oids.len() + 2;

    // let obj = oids
    //     .iter()
    //     .filter(|oid| odb.read_header(**oid).unwrap().1 == ObjectType::Commit)
    //     .next()
    //     .unwrap();

    // println!(
    //     "{}\n{}",
    //     obj,
    //     String::from_utf8(odb.read(*obj).unwrap().data().to_vec()).unwrap()
    // );

    // exit(0);

    let objects = oids.into_iter().map(|oid| {
        let object = odb.read(oid).context(crate::error::Git)?;
        let data = object.data().to_vec();
        let len = data.len();
        let mut compressed = Vec::<u8>::new();
        let encoded = git_object_format::encode(
            std::io::Cursor::new(data),
            crate::git::into_object_type(object.kind()),
        )
        .context(error::Io)?;

        ZlibEncoder::new(encoded, flate2::Compression::best())
            .read_to_end(&mut compressed)
            .context(error::Io)?;

        let hash = oid.to_string();
        let path = format!("/objects/{}/{}", &hash[..2], &hash[2..]);
        Ok((path, compressed))
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
            generate_ref(repo.head().context(error::Git)?)?.into_bytes(),
        ))
    });

    let prefix = git::gen_temp_dir_path();
    let pb = ProgressBar::new(n.try_into().unwrap());
    let mut futures = stream::iter(objects.chain(info_refs).chain(head))
        .map(|res| async {
            match res {
                Err(e) => Err(e),
                Ok((path, data)) => {
                    ipfs::write_file(&ipfs, format!("/{}/{}", prefix, path), data)
                        .map_ok(|_| {
                            pb.inc(1);
                        })
                        .await
                }
            }
        })
        .buffer_unordered(QUEUE_SIZE);

    while let Some(x) = futures.next().await {
        match x {
            Err(e) => panic!("{:?}", e),
            Ok(_) => continue,
        }
    }

    pb.finish_with_message(format!("Finished in {:?}", pb.elapsed()));

    match ipfs.files_stat(format!("/{}", prefix).as_str()).await {
        Err(e) => panic!("{}", e),
        Ok(res) => {
            println!("{}", res.hash);
        }
    }
}
