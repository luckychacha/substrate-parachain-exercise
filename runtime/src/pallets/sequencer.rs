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
	type MinSequencerCount = frame_support::traits::ConstU32<3>;
	// type ValidatorId = <Self as frame_system::Config>::AccountId;
	// type ValidatorIdOf = pallet_collator_selection::IdentityCollator;
}
