use octocrab::models::pulls::PullRequest;

#[derive(Debug)]
pub struct MergeCandidate {
    pub pull: octocrab::models::pulls::PullRequest,
}

impl MergeCandidate {
    #[must_use] pub fn new(pull: PullRequest) -> MergeCandidate {
        MergeCandidate { pull }
    }

    #[must_use] pub fn retarget(self) -> MergeCandidate {
        MergeCandidate { pull: self.pull, }
    }

    pub fn merge(self) {

    }
}