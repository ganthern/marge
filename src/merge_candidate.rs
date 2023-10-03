use octocrab::models::pulls::PullRequest;

enum MergeCandidateState {
    MergeCandidateNew,
    MergeCandidateRetargeted,
    MergeCandidateCheckedOut,
    MergeCandidateRebased,
    MergeCandidateValidated,
    MergeCandidatePushed,
}

pub type Successor = Option<Box<MergeCandidate>>;

#[derive(Debug)]
pub struct MergeCandidate {
    pub pull: octocrab::models::pulls::PullRequest,
    successor: Successor,
}

impl MergeCandidate {
    pub fn new(pull: PullRequest) -> MergeCandidate {
        MergeCandidate {
            pull,
            successor: None,
        }
    }

    pub fn link(&mut self, successor: Option<MergeCandidate>) {
        self.successor = successor.map(|p| Box::new(p))
    }

    pub fn retarget(self) -> MergeCandidate {
        MergeCandidate {
            pull: self.pull,
            successor: self.successor,
        }
    }
}