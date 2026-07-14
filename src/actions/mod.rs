//! Multi-step Discord actions that get triggered from more than one
//! place — a manual slash command *and* an automatic response to a
//! detector, in `lockdown`'s case. This is deliberately empty until now:
//! a one-line HTTP call doesn't need its own module just because it's
//! "an action" — see `01_ARCHITECTURE.md` for why this wasn't created
//! earlier and why lockdown is the first thing that qualifies.

pub mod concurrent;
pub mod lockdown;
