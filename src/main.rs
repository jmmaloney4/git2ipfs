use clap::{crate_authors, crate_description, crate_name, crate_version, App, Arg};
use flate2::read::ZlibEncoder;
use futures::{stream, StreamExt, TryFutureExt};
use git::{all_oids, generate_info_refs, generate_ref};
use git2::Repository;
use indicatif::{ProgressBar, ProgressStyle};
use ipfs_api::IpfsApi;
use snafu::ResultExt;
use std::{
    io::{Cursor, Read},
    iter::once_with,
    path::PathBuf,
};

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

    let pb = ProgressBar::new_spinner();
    let message = format!("Loading git repository at {:#?}...", path);
    pb.set_message(message.clone());

    let ipfs = ipfs_api::IpfsClient::<hyper::client::HttpConnector>::default();
    let repo = Repository::open(path).context(error::Git).unwrap();
    let odb = repo.odb().context(error::Git).unwrap();
    let oids = all_oids(&odb).unwrap();
    let n = oids.len() + 2;

    let objects = oids.into_iter().map(|oid| {
        pb.println(format!("{}", oid));
        let object = odb.read(oid).context(crate::error::Git)?;
        let data = object.data().to_vec();
        let encoded = Cursor::new(git::prefix_for_object_type(object.kind())?)
            .chain(Cursor::new(format!("{}\0", data.len())))
            .chain(Cursor::new(data));

        let mut compressed = Vec::<u8>::new();
        ZlibEncoder::new(encoded, flate2::Compression::best())
            .read_to_end(&mut compressed)
            .context(error::Io)?;

        let hash = oid.to_string();
        let path = format!("/objects/{}/{}", &hash[..2], &hash[2..]);
        pb.println(format!("{} done.", oid));
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
    let mut futures = stream::iter(objects.chain(info_refs).chain(head))
        .map(|res| async {
            match res {
                Err(e) => Err(e),
                Ok((path, data)) => {
                    let path = format!("/{}/{}", prefix, path);
                    pb.println(format!("Waiting for {}", path));
                    let rv = ipfs::write_file(&ipfs, path.clone(), data)
                        .map_ok(|_| {
                            pb.inc(1);
                        })
                        .await;
                    pb.println(format!("Waiting for {} done.", path));
                    rv
                }
            }
        })
        .buffer_unordered(QUEUE_SIZE);

    pb.finish_with_message(format!("{} Done.", message));
    let pb = ProgressBar::new(n.try_into().unwrap());
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{pos:>7}/{len:7} {bar:40.cyan/blue} {elapsed} {msg}")
            .progress_chars("##-"),
    );

    while let Some(x) = futures.next().await {
        match x {
            Err(e) => panic!("{:?}", e),
            Ok(_) => continue,
        }
    }

    // pb.finish_with_message(format!("Finished in {:?}", pb.elapsed()));
    pb.finish();

    match ipfs.files_stat(format!("/{}", prefix).as_str()).await {
        Err(e) => panic!("{}", e),
        Ok(res) => {
            println!("{}", res.hash);
        }
    }
}
