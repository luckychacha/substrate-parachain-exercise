use crate::*;
use sp_staking::SessionIndex;

parameter_types! {
	// Six sessions in an era (24 hours).
	pub const SessionsPerEra: SessionIndex = 6;
}

impl pallet_sequencer::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type SessionsPerEra = SessionsPerEra;
	type HistoryDepth = frame_support::traits::ConstU32<84>;
}
