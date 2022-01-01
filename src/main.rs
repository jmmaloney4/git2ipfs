use clap::{crate_authors, crate_description, crate_name, crate_version, App, Arg};
use flate2::read::ZlibEncoder;
use futures::{stream, FutureExt, StreamExt, TryFutureExt, TryStreamExt};
use git::{all_oids, generate_info_refs, generate_ref};
use git2::{Odb, Oid, References, Repository};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use ipfs_api::IpfsApi;
use snafu::ResultExt;
use std::{
    io::{Cursor, Read},
    iter::once_with,
    path::PathBuf,
    process::exit,
    sync::Arc,
};
use tempfile::TempDir;
use url::Url;

mod error;
mod git;
mod ipfs;

const QUEUE_SIZE: usize = 256;

#[tokio::main]
async fn main() {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::with_name("arg")
                .required(false)
                .help("Paths or urls to the git repositories to upload")
                .index(1)
                .multiple(true),
        )
        .get_matches();

    let paths: Box<dyn Iterator<Item = Result<git2::Repository, error::Error>>> =
        match matches.values_of("arg") {
            None => Box::new(std::iter::once_with(|| {
                Ok(
                    Repository::open(std::env::current_dir().context(error::Io)?)
                        .context(error::Git)?,
                )
            })),
            Some(args) => Box::new(args.map(|arg| {
                if let Ok(url) = Url::parse(arg) {
                    let tmpdir = TempDir::new().context(error::Io)?;
                    Repository::clone(url.as_str(), tmpdir.into_path()).context(error::Git)
                } else {
                    Repository::open(arg).context(error::Git)
                }
            })),
        };

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    // let mp = MultiProgress::new();

    let mp = Arc::new(MultiProgress::new());

    let mut stream = stream::iter(paths).then(|repo| {
        let pb = ProgressBar::new(1);
        Arc::clone(&mp).add(pb.clone());
        Box::pin(async {
            match repo {
                Ok(repo) => {
                    let path = format!("{:?}", repo.path());
                    let rv = git2ipfs(repo, &ipfs, pb).await;
                    Ok((path, rv?))
                }
                Err(e) => Err(e),
            }
        })
    });

    while let Some(res) = stream.next().await {
        match res {
            Err(e) => panic!("{}", e),
            Ok((p, s)) => println!("{:?}: {}", p, s),
        }
    }
}

async fn git2ipfs(
    repo: git2::Repository,
    ipfs: &impl IpfsApi,
    pb: ProgressBar,
) -> Result<String, error::Error> {
    let odb = repo.odb().context(error::Git)?;
    let oids = all_oids(&odb)?;

    let iter = files::objects(oids.into_iter(), &odb)
        .chain(files::info_refs(repo.references().context(error::Git)?))
        .chain(files::head(repo.head().context(error::Git)?));
    pb.set_length(iter.size_hint().0.try_into().unwrap_or_else(|_| todo!()));

    let prefix = git::gen_temp_dir_path();
    let mut futures = stream::iter(iter)
        .map(|res| async {
            match res {
                Err(e) => Err(e),
                Ok((path, data)) => {
                    let path = format!("/{}/{}", prefix, path);
                    pb.println(format!("Started writing {}", &path));
                    let rv = ipfs::write_file(ipfs, path.clone(), data).await;
                    pb.println(format!("Done writing {}", &path));
                    rv
                }
            }
        })
        .buffer_unordered(QUEUE_SIZE);

    while let Some(x) = futures.next().await {
        match x {
            Err(e) => return Err(e),
            Ok(_) => {
                pb.inc(1);
            }
        }
    }

    let rv = match ipfs.files_stat(format!("/{}", prefix).as_str()).await {
        Err(e) => panic!("{}", e),
        Ok(res) => res.hash,
    };

    pb.finish();

    Ok(rv)
}

mod files {
    use std::io::{Cursor, Read};
    use snafu::ResultExt;

    use crate::error::*;
    use crate::git;

    pub(crate) fn objects<'a>(
        oids: impl Iterator<Item = git2::Oid> + 'a,
        odb: &'a git2::Odb,
    ) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), Error>> + 'a> {
        Box::new(oids.map(move |oid| {
            let object = odb.read(oid).context(Git)?;
            let data = object.data().to_vec();

            // Add appropriate header to git object
            let encoded = Cursor::new(git::prefix_for_object_type(object.kind())?)
                .chain(Cursor::new(format!("{}\0", data.len())))
                .chain(Cursor::new(data));

            // Compress object with zlib
            let mut compressed = Vec::<u8>::new();
            flate2::read::ZlibEncoder::new(encoded, flate2::Compression::best())
                .read_to_end(&mut compressed)
                .context(Io)?;

            let hash = oid.to_string();
            let path = format!("/objects/{}/{}", &hash[..2], &hash[2..]);
            Result::<(String, Vec<u8>), Error>::Ok((path, compressed))
        }))
    }

    pub(crate) fn info_refs<'a>(
        refs: git2::References<'a>,
    ) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), Error>> + 'a> {
        Box::new(std::iter::once_with(|| {
            Result::<_, Error>::Ok((
                "/info/refs".to_owned(),
                git::generate_info_refs(refs)?.into_bytes(),
            ))
        }))
    }

    pub(crate) fn head<'a>(
        r: git2::Reference<'a>,
    ) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), Error>> + 'a> {
        Box::new(std::iter::once_with(|| {
            Result::<_, Error>::Ok(("/HEAD".to_owned(), git::generate_ref(r)?.into_bytes()))
        }))
    }
}
