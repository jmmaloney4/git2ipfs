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
}

impl Error {
    pub(crate) fn ipfs(e: impl std::fmt::Display) -> Self {
        Self::Ipfs {
            text: e.to_string(),
        }
    }
}