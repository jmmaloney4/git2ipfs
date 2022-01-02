use crate::error::*;
use git2::{Reference, References};
use itertools::Itertools;
use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};

pub(crate) fn generate_info_refs(refs: References) -> Result<String> {
    refs.map(|res| match res {
        Err(e) => Err(Error::Git { source: e }),
        Ok(r) => Ok(r),
    })
    .filter_ok(|r| !r.is_remote())
    .fold(Ok(String::new()), |collected, next| -> Result<String> {
        match (collected, next) {
            (Err(e), _) | (_, Err(e)) => Err(e),
            (Ok(collected), Ok(next)) => {
                let name = match next.name() {
                    None => return Err(RefError::RefHadNoName {}).context(Ref),
                    Some(name) => name,
                };

                let target = match next.target() {
                    None => {
                        return Err(RefError::RefHadNoTarget {
                            name: name.to_string(),
                        })
                        .context(Ref)
                    }
                    Some(target) => target,
                };

                Ok(collected + format!("{}\t{}\n", target, name).as_str())
            }
        }
    })
}

pub(crate) fn gen_temp_dir_path() -> String {
    const TMP_PATH_LEN: usize = 19;
    Alphanumeric.sample_string(&mut thread_rng(), TMP_PATH_LEN)
}

pub(crate) trait ObjectTypeExt {
    fn prefix(&self) -> &'static str;
}

impl ObjectTypeExt for git2::ObjectType {
    fn prefix(&self) -> &'static str {
        match self {
            git2::ObjectType::Any => unimplemented!(),
            git2::ObjectType::Commit => "commit ",
            git2::ObjectType::Tree => "tree ",
            git2::ObjectType::Blob => "blob ",
            git2::ObjectType::Tag => "tag ",
        }
    }
}
