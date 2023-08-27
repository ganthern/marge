use octocrab::models::pulls::PullRequest;

pub trait MergeCandidateState {}

#[derive(Debug)]
pub enum MergeCandidateNew {}
#[derive(Debug)]
pub enum MergeCandidateRetargeted {}
#[derive(Debug)]
pub enum MergeCandidateCheckedOut {}
#[derive(Debug)]
pub enum MergeCandidateRebased {}
#[derive(Debug)]
pub enum MergeCandidateValidated {}
#[derive(Debug)]
pub enum MergeCandidatePushed {}

impl MergeCandidateState for MergeCandidateNew {}
impl MergeCandidateState for MergeCandidateRetargeted {}
impl MergeCandidateState for MergeCandidateCheckedOut {}
impl MergeCandidateState for MergeCandidateRebased {}
impl MergeCandidateState for MergeCandidateValidated {}
impl MergeCandidateState for MergeCandidatePushed {}

pub type Successor = Option<Box<MergeCandidateNew>>;

#[derive(Debug)]
pub struct MergeCandidate<'a, S: MergeCandidateState + ?Sized> {
    pub pull: octocrab::models::pulls::PullRequest,
    successor: Successor,
    _marker: std::marker::PhantomData<&'a S>,
}

impl<'a> MergeCandidate<'a, MergeCandidateNew> {
    pub fn new(pull: PullRequest) -> MergeCandidate<'a, MergeCandidateNew> {
        MergeCandidate::<MergeCandidateNew> {
            pull,
            successor: None,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn link(&mut self, successor: Option<MergeCandidateNew>) {
        self.successor = successor.map(|p| Box::new(p))
    }

    pub fn retarget(self) -> MergeCandidate<'a, MergeCandidateRetargeted> {
        MergeCandidate {
            pull: self.pull,
            successor: self.successor,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a> MergeCandidate<'a, MergeCandidateRetargeted> {}
