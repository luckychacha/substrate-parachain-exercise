#![cfg_attr(not(feature = "std"), no_std)]
#![recursion_limit = "256"]

use parity_scale_codec::{Decode, Encode, MaxEncodedLen};

use scale_info::TypeInfo;
use sp_runtime::{RuntimeDebug, Saturating};
use sp_staking::EraIndex;

use sp_std::prelude::*;

/// Mode of era-forcing.
#[derive(
	Copy,
	Clone,
	PartialEq,
	Eq,
	Encode,
	Decode,
	RuntimeDebug,
	TypeInfo,
	MaxEncodedLen,
	serde::Serialize,
	serde::Deserialize,
)]
pub enum Forcing {
	/// Not forcing anything - just let whatever happen.
	NotForcing,
	/// Force a new era, then reset to `NotForcing` as soon as it is done.
	/// Note that this will force to trigger an election until a new era is triggered, if the
	/// election failed, the next session end will trigger a new election again, until success.
	ForceNew,
	/// Avoid a new era indefinitely.
	ForceNone,
	/// Force a new era at the end of all sessions indefinitely.
	ForceAlways,
}

impl Default for Forcing {
	fn default() -> Self {
		Forcing::NotForcing
	}
}

/// Information regarding the active era (era in used in session).
#[derive(Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct ActiveEraInfo {
	/// Index of era.
	pub index: EraIndex,
	/// Moment of start expressed as millisecond from `$UNIX_EPOCH`.
	///
	/// Start can be none if start hasn't been set for the era yet,
	/// Start is set on the first on_finalize of the era to guarantee usage of `Time`.
	pub start: Option<u64>,
}
