use git2::{Odb, Oid, Reference, References};
use itertools::Itertools;

use crate::error::*;
use snafu::{OptionExt, ResultExt};

/// Return the object ids for all objects in the object database.
pub(crate) fn all_oids(odb: &Odb) -> Result<Vec<Oid>, Error> {
    let mut ids = Vec::<Oid>::new();
    odb.foreach(|oid| {
        ids.push(*oid);
        true
    })
    .context(Git)?;
    Ok(ids)
}

pub(crate) fn generate_info_refs(refs: References) -> Result<String, Error> {
    refs.map(|res| match res {
        Err(e) => Err(Error::Git { source: e }),
        Ok(r) => Ok(r),
    })
    .filter_ok(|r| !r.is_remote())
    .fold(Ok(String::new()), |x, y| -> Result<String, Error> {
        match (x, y) {
            (Err(e), _) | (_, Err(e)) => Err(e),
            (Ok(x), Ok(y)) => {
                let name = match y.name() {
                    None => return Err(RefError::RefHadNoName {}).context(Ref),
                    Some(name) => name,
                };

                let target = match y.target() {
                    None => {
                        return Err(RefError::RefHadNoTarget {
                            name: name.to_string(),
                        })
                        .context(Ref)
                    }
                    Some(target) => target,
                };

                Ok(x + format!("{}\t{}\n", target, name).as_str())
            }
        }
    })
}

pub(crate) fn generate_ref(reference: Reference) -> Result<String, Error> {
    match reference.kind().context(crate::error::NoReferenceKind)? {
        git2::ReferenceType::Direct => match reference.target() {
            None => unreachable!(),
            Some(oid) => Ok(format!("{}\n", oid)),
        },
        git2::ReferenceType::Symbolic => match reference.symbolic_target_bytes() {
            None => unreachable!(),
            Some(target) => Ok(String::from_utf8(target.to_owned()).context(FromUtf8)?),
        },
    }
}

use rand::distributions::{Alphanumeric, DistString};
use rand::thread_rng;
pub(crate) fn gen_temp_dir_path() -> String {
    const TMP_PATH_LEN: usize = 19;
    Alphanumeric.sample_string(&mut thread_rng(), TMP_PATH_LEN)
}

pub(crate) fn into_object_type(ty: git2::ObjectType) -> git_object_format::Type {
    match ty {
        git2::ObjectType::Any => unimplemented!(),
        git2::ObjectType::Commit => git_object_format::Type::Commit,
        git2::ObjectType::Tree => git_object_format::Type::Tree,
        git2::ObjectType::Blob => git_object_format::Type::Blob,
        git2::ObjectType::Tag => git_object_format::Type::Tag,
    }
}
