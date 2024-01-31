#![cfg_attr(not(feature = "std"), no_std)]

use codec::FullCodec;
use ep_sequencer::Forcing;
use frame_support::sp_runtime::traits::{AccountIdConversion, Get};
use frame_support::{BoundedVec, PalletId};
pub use pallet::*;
use sp_core::ConstU32;
use sp_runtime::traits::Bounded;
use sp_std::vec::Vec;
use sp_staking::{EraIndex, SessionIndex};

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;
	use ep_sequencer::{ActiveEraInfo, Forcing};
	use sp_staking::{EraIndex, SessionIndex};

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Number of sessions per era.
		#[pallet::constant]
		type SessionsPerEra: Get<SessionIndex>;

		/// Number of eras to keep in history.
		#[pallet::constant]
		type HistoryDepth: Get<u32>;

		#[pallet::constant]
		type MinSequencerCount: Get<u32>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// The current era index.
	///
	/// This is the latest planned era, depending on how the Session pallet queues the validator
	/// set, it might be active or not.
	#[pallet::storage]
	#[pallet::getter(fn current_era)]
	pub type CurrentEra<T> = StorageValue<_, EraIndex>;

	/// The active era information, it holds index and start.
	///
	/// The active era is the era being currently rewarded. Validator set of this era must be
	/// equal to [`SessionInterface::validators`].
	#[pallet::storage]
	#[pallet::getter(fn active_era)]
	pub type ActiveEra<T> = StorageValue<_, ActiveEraInfo>;

	/// The session index at which the era start for the last [`Config::HistoryDepth`] eras.
	///
	/// Note: This tracks the starting session (i.e. session index when era start being active)
	/// for the eras in `[CurrentEra - HISTORY_DEPTH, CurrentEra]`.
	#[pallet::storage]
	#[pallet::getter(fn eras_start_session_index)]
	pub type ErasStartSessionIndex<T> = StorageMap<_, Twox64Concat, EraIndex, SessionIndex>;

	/// Mode of era forcing.
	#[pallet::storage]
	#[pallet::getter(fn force_era)]
	pub type ForceEra<T> = StorageValue<_, Forcing, ValueQuery>;

	#[pallet::storage]
	#[pallet::unbounded]
	pub type ErasSequencers<T: Config> = StorageMap<
		_,
		Twox64Concat,
		EraIndex,
		BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::unbounded]
	pub type RestakeData<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		u128,
		ValueQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {}

	#[pallet::error]
	pub enum Error<T> {
		NoneValue,
		StorageOverflow,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {}
}

impl <T: Config> Pallet<T> {

	/// Clear all era information for given era.
	pub(crate) fn clear_era_information(era_index: EraIndex) {
		ErasStartSessionIndex::<T>::remove(era_index);
	}

	fn try_trigger_new_era(start_session_index: SessionIndex, validators: &Vec<T::AccountId>) -> Option<BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>>> {
		match CurrentEra::<T>::get() {
			None => {
				CurrentEra::<T>::put(0);
				ErasStartSessionIndex::<T>::insert(&0, &start_session_index);
			},
			_ => (),
		}
		
		Self::trigger_new_era(start_session_index, validators)
	}

	fn trigger_new_era(start_session_index: SessionIndex, validators: &Vec<T::AccountId>) -> Option<BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>>> {
		let new_planned_era = CurrentEra::<T>::mutate(|s| {
			*s = Some(s.map(|s| s + 1).unwrap_or(0));
			s.unwrap()
		});

		ErasStartSessionIndex::<T>::insert(&new_planned_era, &start_session_index);

		if let Some(old_era) = new_planned_era.checked_sub(T::HistoryDepth::get()) {
			Self::clear_era_information(old_era);
		}

		// let mut validators_vec = Vec::new();
		// for validator in validators {
		// 	validators_vec.push(validator.clone());
		// }

		// todo: iter RestakeData and order by restake amount descending and compare with validators_vec to filter at least MinSequencerCount validators
		let min_sequencers = T::MinSequencerCount::get() as usize;
		let (total_stake, num_stakers) = RestakeData::<T>::iter()
			.fold((0u128, 0usize), |(total_stake, count), (_, stake)| {
				(total_stake + stake, count + 1)
			});

		let average_stake = if num_stakers > 0 { total_stake / num_stakers as u128 } else { 0 };

		// 2. filter amount greater than avg's 2/3 validators
		let two_thirds_average = (average_stake * 2) / 3;
		let mut sequencers = Vec::new();
	
		for validator in validators {
			if RestakeData::<T>::get(validator) >= two_thirds_average {
				sequencers.push(validator.clone());
			}
		}
	
		// 3. 如果选出的sequencer数量小于min_sequencers，继续添加validators
		if sequencers.len() < min_sequencers {
			for validator in validators {
				if !sequencers.contains(validator) {
					sequencers.push(validator.clone());
					if sequencers.len() >= min_sequencers {
						break;
					}
				}
			}
		}

		let bounded_sequencers: BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>> = sequencers.try_into().expect("too many validators");

		EraInfo::<T>::set_sequencer(start_session_index, bounded_sequencers.clone());

		Some(bounded_sequencers)
	}

	fn new_session(session_index: SessionIndex, validators: &Vec<T::AccountId>) -> Option<BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>>> {
		if let Some(current_era) = Self::current_era() {
			let current_era_start_session_index = Self::eras_start_session_index(current_era).unwrap_or_else(|| {
				frame_support::print("Error: start_session_index must be set for current_era");
				0
			});

			let era_length = session_index.saturating_sub(current_era_start_session_index); // Must never happen.

			match ForceEra::<T>::get() {
				// Will be set to `NotForcing` again if a new era has been triggered.
				Forcing::ForceNew => (),
				// Short circuit to `try_trigger_new_era`.
				Forcing::ForceAlways => (),
				// Only go to `try_trigger_new_era` if deadline reached.
				Forcing::NotForcing if era_length >= T::SessionsPerEra::get() => (),
				_ => {
					return None
				},
			}

			// New Era
			let maybe_new_era_validators = Self::try_trigger_new_era(session_index, validators);
			maybe_new_era_validators
		} else {
			// Set initial era.
			log::info!("Starting the first era.");
			Self::try_trigger_new_era(session_index, validators)
		}
	}
}

/// Wrapper struct for Era related information. It is not a pure encapsulation as these storage
/// items can be accessed directly but nevertheless, its recommended to use `EraInfo` where we
/// can and add more functions to it as needed.
pub struct EraInfo<T>(sp_std::marker::PhantomData<T>);

impl<T: Config> EraInfo<T> {
	/// Store exposure for elected sequencers at start of an era.
	pub fn set_sequencer(
		era: EraIndex,
		sequencers: BoundedVec<T::AccountId, ConstU32<{ u32::MAX }>>,
	) {
		<ErasSequencers<T>>::insert(era, &sequencers);
	}
}

pub struct SessionManager<I, T>(sp_std::marker::PhantomData<(I, T)>);

impl<I, T> pallet_session::SessionManager<<T as frame_system::Config>::AccountId> for SessionManager<I, T>
where
	I: pallet_session::SessionManager<<T as frame_system::Config>::AccountId>,
	T: Config,
{
    fn new_session(new_index: SessionIndex) -> Option<Vec<<T as frame_system::Config>::AccountId>> {
        let new_session = I::new_session(new_index);
		// log validators
        if let Some(validators) = &new_session {
			Pallet::<T>::new_session(new_index, validators);
        }

        new_session
    }

    fn new_session_genesis(new_index: SessionIndex) -> Option<Vec<<T as frame_system::Config>::AccountId>> {
        I::new_session_genesis(new_index)
    }

    fn end_session(end_index: SessionIndex) { I::end_session(end_index); }
    fn start_session(start_index: SessionIndex) {
		I::start_session(start_index);
		// Update ActiveEra if start_index == ErasStartSessionIndex of CurrentEra.
	}
}
