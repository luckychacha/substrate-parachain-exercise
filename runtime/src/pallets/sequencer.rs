use crate::*;
use polkadot_runtime_common::prod_or_fast;

parameter_types! {
	// Six sessions in an era (24 hours).
	pub const SessionsPerEra: SessionIndex = prod_or_fast!(6, 1);
}

impl pallet_sequencer::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type SessionsPerEra = SessionsPerEra;
}
