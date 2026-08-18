#![expect(
    unreachable_pub,
    reason = "internal reference modules share narrow items without exporting them from the crate"
)]

pub mod bootstrap;
pub(crate) mod build;
pub mod helpers;
pub(crate) mod inspect;
pub(crate) mod lock;
pub(crate) mod provenance;
pub(crate) mod reconcile;
pub(crate) mod stage;
pub mod storage;
pub(crate) mod tree;
