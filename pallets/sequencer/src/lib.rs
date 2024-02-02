#![cfg_attr(not(feature = "std"), no_std)]

use codec::FullCodec;
use ep_sequencer::{ActiveEraInfo, Forcing};
use frame_support::traits::{Get, UnixTime};
use frame_support::BoundedVec;
pub use pallet::*;
use sp_core::ConstU32;
use sp_staking::{EraIndex, SessionIndex};
use sp_std::vec::Vec;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

type Sequencer<T> = (<T as frame_system::Config>::AccountId, u128);

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::SaturatedConversion;

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

		/// Time used for computing era duration.
		///
		/// It is guaranteed to start being called from the first `on_finalize`. Thus value at
		/// genesis is not used.
		type UnixTime: UnixTime;
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

	/// Sequencers per group
	#[pallet::storage]
	#[pallet::getter(fn sequencers_per_group)]
	pub type SequencersPerGroup<T> = StorageValue<_, u32, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn eras_sequencers)]
	pub type ErasSequencers<T: Config> = StorageMap<
		_,
		Twox64Concat,
		EraIndex,
		BoundedVec<Sequencer<T>, ConstU32<{ u32::MAX }>>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn restake_data)]
	pub type RestakeData<T: Config> = StorageMap<_, Twox64Concat, T::AccountId, u128, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {}

	#[pallet::error]
	pub enum Error<T> {
		NoneValue,
		StorageOverflow,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_finalize(_n: BlockNumberFor<T>) {
			// Set the start of the first era.
			if let Some(mut active_era) = Self::active_era() {
				if active_era.start.is_none() {
					let now_as_millis_u64 = T::UnixTime::now().as_millis().saturated_into::<u64>();
					active_era.start = Some(now_as_millis_u64);
					// This write only ever happens once, we don't include it in the weight in
					// general
					ActiveEra::<T>::put(active_era);
				}
			}
			// `on_finalize` weight is tracked in `on_initialize`
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		// Temp Set RestakeData
		#[pallet::weight({0})]
		#[pallet::call_index(0)]
		pub fn deposit(
			_origin: OriginFor<T>,
			account_id: T::AccountId,
			amount: u128,
		) -> DispatchResultWithPostInfo {
			RestakeData::<T>::insert(&account_id, amount);
			Ok(().into())
		}


		// Set SequencersPerGroup
		#[pallet::weight({1})]
		#[pallet::call_index(1)]
		pub fn set_sequencers_per_group(
			origin: OriginFor<T>,
			sequencers_per_group: u32,
		) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;
			SequencersPerGroup::<T>::put(sequencers_per_group);
			Ok(().into())
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Clear all era information for given era.
	pub(crate) fn clear_era_information(era_index: EraIndex) {
		ErasStartSessionIndex::<T>::remove(era_index);
	}

	fn try_trigger_new_era(
		start_session_index: SessionIndex,
		validators: &Vec<T::AccountId>,
	) -> Option<BoundedVec<Sequencer<T>, ConstU32<{ u32::MAX }>>> {
		match CurrentEra::<T>::get() {
			None => {
				CurrentEra::<T>::put(0);
				ErasStartSessionIndex::<T>::insert(&0, &start_session_index);
				log::info!("Updated current_era to: 0");
			},
			_ => (),
		}

		Self::trigger_new_era(start_session_index, validators)
	}

	fn trigger_new_era(
		start_session_index: SessionIndex,
		validators: &Vec<T::AccountId>,
	) -> Option<BoundedVec<Sequencer<T>, ConstU32<{ u32::MAX }>>> {
		let new_planned_era = CurrentEra::<T>::mutate(|s| {
			*s = Some(s.map(|s| s + 1).unwrap_or(0));
			s.unwrap()
		});

		ErasStartSessionIndex::<T>::insert(&new_planned_era, &start_session_index);

		if let Some(old_era) = new_planned_era.checked_sub(T::HistoryDepth::get()) {
			Self::clear_era_information(old_era);
		}

		let min_sequencers = T::MinSequencerCount::get() as usize;
		let (total_stake, num_stakers) = RestakeData::<T>::iter()
			.fold((0u128, 0usize), |(total_stake, count), (_, stake)| {
				(total_stake + stake, count + 1)
			});

		let average_stake = if num_stakers > 0 { total_stake / num_stakers as u128 } else { 0 };

		// 2. filter amount greater than avg's 2/3 validators
		let two_thirds_average = {
			let temp = average_stake * 2;
			if temp >= 3 {
				temp / 3
			} else {
				1
			}
		};
		let mut sequencers = Vec::new();

		for validator in validators {
			let stake = RestakeData::<T>::get(validator);
			if stake >= two_thirds_average {
				sequencers.push((validator.clone(), stake));
			}
		}

		// 3. if sequencer amount less than min_sequencers，add more validators to sequencers until sequencers.len() >= min_sequencers
		if sequencers.len() < min_sequencers {
			for validator in validators {
				if !sequencers.iter().any(|(v, _)| v == validator) {
					let stake = RestakeData::<T>::get(validator);
					sequencers.push((validator.clone(), stake));
					if sequencers.len() >= min_sequencers {
						break;
					}
				}
			}
		}

		let bounded_sequencers: BoundedVec<Sequencer<T>, ConstU32<{ u32::MAX }>> =
			sequencers.try_into().expect("too many validators");

		EraInfo::<T>::set_sequencer(new_planned_era, bounded_sequencers.clone());

		log::info!(
			"New era #{} has started at session {}",
			new_planned_era,
			start_session_index,
		);

		Some(bounded_sequencers)
	}

	fn new_session(
		session_index: SessionIndex,
		validators: &Vec<T::AccountId>,
	) -> Option<BoundedVec<Sequencer<T>, ConstU32<{ u32::MAX }>>> {
		if let Some(current_era) = Self::current_era() {
			let current_era_start_session_index = Self::eras_start_session_index(current_era)
				.unwrap_or_else(|| {
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
				_ => return None,
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

	/// Start a session potentially starting an era.
	fn start_session(start_session: SessionIndex) {
		let next_active_era = Self::active_era().map(|e| e.index + 1).unwrap_or(0);

		if let Some(next_active_era_start_session_index) =
			Self::eras_start_session_index(next_active_era)
		{
			if next_active_era_start_session_index == start_session {
				Self::start_era();
			} else if next_active_era_start_session_index < start_session {
				// This arm should never happen, but better handle it than to stall the staking
				// pallet.
				frame_support::print("Warning: A session appears to have been skipped.");
				Self::start_era();
			}
		}
	}

	/// Start a new era. It does:
	/// * Increment `active_era.index`,
	/// * reset `active_era.start`,
	fn start_era() {
		ActiveEra::<T>::mutate(|active_era| {
			let new_index = active_era.as_ref().map(|info| info.index + 1).unwrap_or(0);
			*active_era = Some(ActiveEraInfo {
				index: new_index,
				// Set new active era start in next `on_finalize`. To guarantee usage of `Time`
				start: None,
			});
		});
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
		sequencers: BoundedVec<(T::AccountId, u128), ConstU32<{ u32::MAX }>>,
	) {
		<ErasSequencers<T>>::insert(era, &sequencers);
	}
}

pub struct SessionManager<I, T>(sp_std::marker::PhantomData<(I, T)>);

impl<I, T> pallet_session::SessionManager<<T as frame_system::Config>::AccountId>
	for SessionManager<I, T>
where
	I: pallet_session::SessionManager<<T as frame_system::Config>::AccountId>,
	T: Config,
{
	fn new_session(new_index: SessionIndex) -> Option<Vec<<T as frame_system::Config>::AccountId>> {
		let new_session = I::new_session(new_index);
		if let Some(validators) = &new_session {
			Pallet::<T>::new_session(new_index, validators);
		}
		new_session
	}

	fn new_session_genesis(
		new_index: SessionIndex,
	) -> Option<Vec<<T as frame_system::Config>::AccountId>> {
		let new_session = I::new_session_genesis(new_index);
		if let Some(validators) = &new_session {
			Pallet::<T>::new_session(new_index, validators);
		}
		new_session
	}

	fn end_session(end_index: SessionIndex) {
		I::end_session(end_index);
	}
	fn start_session(start_index: SessionIndex) {
		I::start_session(start_index);
		Pallet::<T>::start_session(start_index);
	}
}
