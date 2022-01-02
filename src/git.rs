use crate::error::*;
use git2::{Reference, References};
use itertools::Itertools;
use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};
use snafu::{OptionExt, ResultExt};

pub(crate) fn generate_info_refs(refs: References) -> Result<String, Error> {
    refs.map(|res| match res {
        Err(e) => Err(Error::Git { source: e }),
        Ok(r) => Ok(r),
    })
    .filter_ok(|r| !r.is_remote())
    .fold(
        Ok(String::new()),
        |collected, next| -> Result<String, Error> {
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
        },
    )
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

pub(crate) fn gen_temp_dir_path() -> String {
    const TMP_PATH_LEN: usize = 19;
    Alphanumeric.sample_string(&mut thread_rng(), TMP_PATH_LEN)
}

pub(crate) fn prefix_for_object_type(ty: git2::ObjectType) -> Result<&'static [u8], Error> {
    match ty {
        git2::ObjectType::Any => unimplemented!(),
        git2::ObjectType::Commit => Ok("commit ".as_bytes()),
        git2::ObjectType::Tree => Ok("tree ".as_bytes()),
        git2::ObjectType::Blob => Ok("blob ".as_bytes()),
        git2::ObjectType::Tag => Ok("tag ".as_bytes()),
    }
}
