use snafu::Snafu;

#[derive(Debug, Snafu)]
pub(crate) enum RefError {
    #[snafu()]
    RefHadNoName {},

    #[snafu()]
    RefHadNoTarget { name: String },
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub(crate) enum Error {
    #[snafu(display("Ipfs API Error: {}", text))]
    Ipfs { text: String },

    #[snafu(display("Git Error: {}", source))]
    Git { source: git2::Error },

    #[snafu(display("Parse Int Error {:?}", source))]
    Parse { source: core::num::ParseIntError },

    #[snafu(display("Mismatched Sizes: {} (data) ≠ {} (ipfs)", data, ipfs))]
    MismatchedSizes { data: usize, ipfs: usize },

    #[snafu(display("Cid Error: {}", source))]
    Cid { source: cid::Error },

    #[snafu(display("Ref Error: {}", source))]
    Ref { source: RefError },

    #[snafu(display("Couldn't determine reference kind"))]
    NoReferenceKind {},

    #[snafu(display("Failed to convert bytes to utf8: {}", source))]
    FromUtf8 { source: std::string::FromUtf8Error },

    #[snafu(display("Io Error {}", source))]
    Io { source: std::io::Error },

    #[snafu(display("Custom Error {}", text))]
    Custom { text: String },
}

impl Error {
    pub(crate) fn ipfs(e: impl std::fmt::Display) -> Self {
        Self::Ipfs {
            text: e.to_string(),
        }
    }

    pub(crate) fn custom(e: impl std::fmt::Display) -> Self {
        Self::Custom {
            text: e.to_string(),
        }
    }
}
