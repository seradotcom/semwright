//! Effect evaluation is data processing, never execution or authorization.
//! A owns the shared reports, verdicts, bases, effects and PlanVault.
mod contract;
mod predicate;
pub use contract::*;
pub use predicate::*;
pub use semwright_semantic_composition as composition;

mod enumeration;
mod evidence;
mod evaluator;
pub use enumeration::*;
pub use evidence::*;
pub use evaluator::*;

mod quality;
pub use quality::*;
