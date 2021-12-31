use clap::{crate_authors, crate_description, crate_name, crate_version, App, Arg};
use flate2::read::ZlibEncoder;
use futures::{stream, StreamExt, TryFutureExt};
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
};
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

    let paths: Box<dyn Iterator<Item = PathBuf>> = match matches.values_of("arg") {
        None => Box::new(std::iter::once(
            std::env::current_dir().unwrap_or_else(|_| panic!("Couldn't get current directory.")),
        )),
        Some(args) => Box::new(args.map(|arg| {
            if let Ok(url) = Url::parse(arg) {
                todo!()
            } else {
                PathBuf::from(arg)
            }
        })),
    };

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let mp = MultiProgress::new();

    let mut stream = stream::iter(paths).then(|path| {
        let pb = ProgressBar::new(0);
        mp.add(pb.clone());
        Box::pin(git2ipfs(path, &ipfs, pb))
    });

    while let Some(res) = stream.next().await {
        match res {
            Err(e) => panic!("{}", e),
            Ok((p, s)) => println!("{:?}: {}", p, s),
        }
    }
}

async fn git2ipfs(
    path: PathBuf,
    ipfs: &impl IpfsApi,
    pb: ProgressBar,
) -> Result<(PathBuf, String), error::Error> {
    let repo = Repository::open(&path).context(error::Git).unwrap();
    let odb = repo.odb().context(error::Git).unwrap();
    let oids = all_oids(&odb).unwrap();

    let iter = object_iter(oids.into_iter(), &odb)
        .chain(info_refs(repo.references().context(error::Git)?))
        .chain(head(repo.head().context(error::Git)?));
    println!("{}", iter.size_hint().0);
    pb.set_length(iter.size_hint().0.try_into().unwrap_or_else(|_| todo!()));

    let prefix = git::gen_temp_dir_path();
    let mut futures = stream::iter(iter)
        .map(|res| async {
            match res {
                Err(e) => Err(e),
                Ok((path, data)) => {
                    let path = format!("/{}/{}", prefix, path);
                    let rv = ipfs::write_file(ipfs, path.clone(), data).await;
                    rv
                }
            }
        })
        .buffer_unordered(QUEUE_SIZE);

    while let Some(x) = futures.next().await {
        match x {
            Err(e) => return Err(e),
            Ok(_) => pb.inc(1),
        }
    }

    let rv = match ipfs.files_stat(format!("/{}", prefix).as_str()).await {
        Err(e) => panic!("{}", e),
        Ok(res) => res.hash,
    };

    pb.finish();

    Ok((path, rv))
}

fn object_iter<'a>(
    oids: impl Iterator<Item = Oid> + 'a,
    odb: &'a Odb,
) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), error::Error>> + 'a> {
    Box::new(oids.map(move |oid| {
        let object = odb.read(oid).context(crate::error::Git)?;
        let data = object.data().to_vec();

        // Add appropriate header to git object
        let encoded = Cursor::new(git::prefix_for_object_type(object.kind())?)
            .chain(Cursor::new(format!("{}\0", data.len())))
            .chain(Cursor::new(data));

        // Compress object with zlib
        let mut compressed = Vec::<u8>::new();
        ZlibEncoder::new(encoded, flate2::Compression::best())
            .read_to_end(&mut compressed)
            .context(error::Io)?;

        let hash = oid.to_string();
        let path = format!("/objects/{}/{}", &hash[..2], &hash[2..]);
        Result::<(String, Vec<u8>), error::Error>::Ok((path, compressed))
    }))
}

fn info_refs<'a>(
    refs: git2::References<'a>,
) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), error::Error>> + 'a> {
    Box::new(once_with(|| {
        Result::<_, error::Error>::Ok((
            "/info/refs".to_owned(),
            generate_info_refs(refs)?.into_bytes(),
        ))
    }))
}

fn head<'a>(
    r: git2::Reference<'a>,
) -> Box<dyn Iterator<Item = Result<(String, Vec<u8>), error::Error>> + 'a> {
    Box::new(once_with(|| {
        Result::<_, error::Error>::Ok(("/HEAD".to_owned(), generate_ref(r)?.into_bytes()))
    }))
}
