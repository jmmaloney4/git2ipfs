use clap::{crate_authors, crate_description, crate_name, crate_version, App, Arg};

use futures::{stream, FutureExt, StreamExt, TryFutureExt};
use git::all_oids;
use git2::Repository;
use indicatif::{MultiProgress, ProgressBar};
use ipfs_api::IpfsApi;
use snafu::{ResultExt};
use std::{io::Read, process::exit, sync::Arc};
use tempfile::TempDir;
use tokio::sync::oneshot;
use url::Url;

mod error;
mod git;
mod ipfs;

const QUEUE_SIZE: usize = 4096;

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

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let mp = Arc::new(MultiProgress::new());

    let paths: Box<dyn Iterator<Item = Result<git2::Repository, error::Error>>> = match matches
        .values_of("arg")
    {
        None => Box::new(std::iter::once_with(|| {
            Repository::open(std::env::current_dir().context(error::Io)?).context(error::Git)
        })),
        Some(args) => Box::new(args.map(|arg| {
            if let Ok(url) = Url::parse(arg) {
                let message = format!("Cloning {}...", url);
                let pb = mp.add(ProgressBar::new_spinner().with_message(message.clone()));
                pb.tick();
                let rv =
                    Repository::clone(url.as_str(), TempDir::new().context(error::Io)?.into_path())
                        .context(error::Git);
                pb.finish_with_message(format!("{} Done.", message));
                rv
            } else {
                Repository::open(arg).context(error::Git)
            }
        })),
    };

    let pb = mp.add(
        ProgressBar::new(paths.size_hint().0.try_into().unwrap_or_else(|_| todo!())).with_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:cyan/blue} {pos:>7}/{len:7} {msg}")
                .progress_chars("##-"),
        ),
    );
    pb.tick();

    let mut stream = stream::iter(paths).then(|repo| {
        Box::pin(async {
            match repo {
                Ok(repo) => {
                    let path = format!("{:?}", repo.path());
                    let rv = git2ipfs(repo, &ipfs, Arc::clone(&mp)).await;
                    Ok((path, rv?))
                }
                Err(e) => Err(e),
            }
        })
    });

    let (joint, mut joinr) = oneshot::channel::<()>();
    let ui = {
        // Spawn ui handling thread
        let mp = Arc::clone(&mp);
        tokio::spawn(async move {
            loop {
                if let Err(e) = mp.join() {
                    break Err(e);
                }
                if let Ok(_) = joinr.try_recv() {
                    break Ok(());
                }
            }
        })
    };

    let mut err = None;
    while let Some(res) = stream.next().await {
        match res {
            Err(e) => err = Some(e),
            Ok((p, s)) => {
                pb.inc(1);
                pb.println(format!("{} {:#}", s, p));
            }
        }
    }

    pb.finish();
    let _ = joint.send(());
    if let Err(e) = ui.await {
        panic!("{}", e);
    }

    if let Some(e) = err {
        eprintln!("An error occurred: {}", e);
        if let Some(backtrace) = snafu::ErrorCompat::backtrace(&e) {
            eprintln!("{}", backtrace);
        }
        exit(exitcode::DATAERR);
    }
    exit(exitcode::OK);
}

async fn git2ipfs(
    repo: git2::Repository,
    ipfs: &impl IpfsApi,
    mp: Arc<MultiProgress>,
) -> Result<String, error::Error> {
    let odb = repo.odb().context(error::Git)?;

    let iter = files::objects(all_oids(&odb)?.into_iter(), &odb)
        .chain(files::info_refs(repo.references().context(error::Git)?))
        .chain(files::head(repo.head().context(error::Git)?));
    let pb = mp.add(
        ProgressBar::new(iter.size_hint().0.try_into().unwrap_or_else(|_| todo!()))
            .with_style(
                indicatif::ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                    .progress_chars("##-"),
            )
            .with_prefix(format!("{:?}", repo.path())),
    );

    let (joint, mut joinr) = oneshot::channel::<()>();
    let join = tokio::spawn({
        let pb = pb.clone();
        async move {
            while let Err(oneshot::error::TryRecvError::Empty) = joinr.try_recv() {
                pb.tick();
            }
        }
    });

    let prefix = git::gen_temp_dir_path();
    let mut futures = stream::iter(iter)
        .map(|res| async {
            match res {
                Err(e) => Err(e),
                Ok((path, data)) => {
                    pb.println(format!("{}", path));
                    let path = format!("/{}/{}", prefix, path);
                    ipfs::write_file(ipfs, path.clone(), data).await
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
        Err(e) => return Err(error::Error::ipfs(e)),
        Ok(res) => res.hash,
    };

    let _ = joint.send(());
    if let Err(e) = join.await {
        return Err(error::Error::custom(e));
    }

    pb.finish();

    Ok(rv)
}

mod files {
    use snafu::ResultExt;
    use std::io::{Cursor, Read};

    use crate::error::*;
    use crate::git;

    type FileInfo = (String, Vec<u8>);
    type FileInfoResult = Result<FileInfo, Error>;

    pub(crate) fn objects<'a>(
        oids: impl Iterator<Item = git2::Oid> + 'a,
        odb: &'a git2::Odb,
    ) -> Box<dyn Iterator<Item = FileInfoResult> + 'a> {
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
            Ok((
                format!("/objects/{}/{}", &hash[..2], &hash[2..]),
                compressed,
            ))
        }))
    }

    pub(crate) fn info_refs<'a>(
        refs: git2::References<'a>,
    ) -> Box<dyn Iterator<Item = FileInfoResult> + 'a> {
        Box::new(std::iter::once_with(|| {
            Ok((
                "/info/refs".to_owned(),
                git::generate_info_refs(refs)?.into_bytes(),
            ))
        }))
    }

    pub(crate) fn head<'a>(
        r: git2::Reference<'a>,
    ) -> Box<dyn Iterator<Item = FileInfoResult> + 'a> {
        Box::new(std::iter::once_with(|| {
            Ok(("/HEAD".to_owned(), git::generate_ref(r)?.into_bytes()))
        }))
    }
}
