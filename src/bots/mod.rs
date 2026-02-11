//! A collection of pre-built bots to test with.

pub mod honest_bot;
pub mod random_bot;
pub mod static_bot;
pub mod duel_bot;

pub use honest_bot::HonestBot;
pub use random_bot::RandomBot;
pub use static_bot::StaticBot;
pub use duel_bot::DuelBot;
