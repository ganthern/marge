use octocrab::models::pulls::PullRequest;

enum MergeCandidateState {
    MergeCandidateNew,
    MergeCandidateRetargeted,
    MergeCandidateCheckedOut,
    MergeCandidateRebased,
    MergeCandidateValidated,
    MergeCandidatePushed,
}

#[derive(Debug)]
pub struct MergeCandidate {
    pub pull: octocrab::models::pulls::PullRequest,
}

impl MergeCandidate {
    pub fn new(pull: PullRequest) -> MergeCandidate {
        MergeCandidate { pull }
    }

    pub fn retarget(self) -> MergeCandidate {
        MergeCandidate { pull: self.pull, }
    }
}