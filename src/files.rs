use crate::{error::*, git::ObjectTypeExt};
use bytes::Bytes;
use itertools::Itertools;
use std::io::Read;

/// A pair of path and data
struct File<P, D> {
    path: P,
    data: D,
}

impl<P, D> File<P, D> {
    fn new(path: P, data: D) -> Self {
        Self { path, data }
    }
}

impl<P> File<P, Bytes> {
    fn from_reference(path: P, reference: git2::Reference) -> Result<Self> {
        let data = match reference.kind().context(crate::error::NoReferenceKind)? {
            git2::ReferenceType::Direct => match reference.target() {
                None => unreachable!(),
                Some(oid) => Bytes::from(format!("{}\n", oid)),
            },
            git2::ReferenceType::Symbolic => match reference.symbolic_target_bytes() {
                None => unreachable!(),
                Some(target) => {
                    let mut buf = Vec::new();
                    "ref: ".as_bytes().chain(target).read_to_end(&mut buf);
                    Bytes::from(buf)
                }
            },
        };

        Ok(File::new(path, data))
    }
}

impl File<String, Bytes> {
    fn from_object<'a>(odb: &'a git2::Odb<'a>, oid: git2::Oid) -> Result<Self> {
        let (reader, len, kind) = odb.reader(oid).context(Git)?;
        let prefix = format!("{}\0", len);

        // Add appropriate header to git object
        let encoded = kind
            .prefix()
            .as_bytes()
            .chain(prefix.as_bytes())
            .chain(reader);

        // Compress object with zlib
        let mut compressed = Vec::new();
        flate2::read::ZlibEncoder::new(encoded, flate2::Compression::best())
            .read_to_end(&mut compressed)
            .context(Io)?;

        let hash = oid.to_string();
        Ok(File::new(
            format!("/objects/{}/{}", &hash[..2], &hash[2..]),
            Bytes::from(compressed),
        ))
    }
}

impl File<&'static str, Bytes> {
    fn info_refs<'a>(references: git2::References<'a>) -> Result<Self> {
        let data = references
            .map(|res| res.context(Git))
            .filter_ok(|r| !r.is_remote())
            .fold(Ok(String::new()), |collected, next| -> Result<String> {
                match (collected, next) {
                    (Err(e), _) | (_, Err(e)) => Err(e),
                    (Ok(collected), Ok(next)) => match next.name() {
                        None => Err(RefError::RefHadNoName {}).context(Ref),
                        Some(name) => match next.target() {
                            None => Err(RefError::RefHadNoTarget {
                                name: name.to_string(),
                            })
                            .context(Ref),
                            Some(target) => {
                                Ok(collected + format!("{}\t{}\n", target, name).as_str())
                            }
                        },
                    },
                }
            })?;
        Ok(File::new("/info/refs", Bytes::from(data)))
    }
}
