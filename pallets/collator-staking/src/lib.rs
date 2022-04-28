#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

mod set;
pub mod types;

pub use pallet::*;
pub use types::*;

#[frame_support::pallet]
pub mod pallet {
	use crate::{
		set::OrderedSet,
		types::{
			Bond, Candidate, CandidateStatus, DelegationCounter, Delegator, DelegatorStatus,
			RoundInfo, BondOf, TotalStake
		},
	};
	use frame_support::{
		dispatch::{DispatchResult, DispatchResultWithPostInfo},
		pallet_prelude::*,
		sp_runtime::{
			traits::{AccountIdConversion, CheckedSub, Convert, Zero},
			RuntimeDebug,
		},
		traits::{
			Currency, EnsureOrigin, ExistenceRequirement, ExistenceRequirement::KeepAlive,
			Randomness, ReservableCurrency, ValidatorRegistration, ValidatorSet,
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::{traits::Saturating, Perquintill};
	use sp_std::{vec, vec::Vec};
	// the round is the session
	pub(crate) type RoundIndex = u32;

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	// this is a collator staking pallet (select collator and reward them)
	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	pub const PALLET_ID: PalletId = PalletId(*b"CollStak");

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		type Currency: Currency<Self::AccountId>;

		/// Origin that can dictate updating parameters of this pallet.
		type UpdateOrigin: EnsureOrigin<Self::Origin>;

		/// max number of fix collators.eg: 5
		// #[pallet::constant]
		type MaxInvulnerables: Get<u32>;


		type MinSelectedCandidates: Get<u32>;

		/// the minimun block length during a round
		type MinLengthPerRound: Get<u32>;

		/// Maximum number of delegations which can be made within the same
		/// round.
		///
		/// NOTE: To prevent re-delegation-reward attacks, we should keep this
		/// to be one.
		// #[pallet::constant]
		type MaxDelegationsPerRound: Get<u32>;

		/// Max delegate numbers of a candidate
		// #[pallet::constant]
		type MaxDelegatorsOfCandidate: Get<u32> + Debug + PartialEq;

		/// Maximum number of collator candidates a delegator can delegate.
		// #[pallet::constant]
		type MaxCandidatesPerDelegator: Get<u32> + Debug + PartialEq;

		/// Minimum staking amount by delegator to delegate to candidate
		/// we don't set the upper limit of delegate amount for once delegate
		// #[pallet::constant]
		type MininumDelegation: BalanceOf<T>;

		/// Minimum stake amount to become a delegator
		// #[pallet::constant]
		type MinimumDelegatorBond: BalanceOf<T>;

		/// Minimum staking amount of candidate to be a collator(this is a limiti for a collator candidate)
		type MinumunCollatorStake: BalanceOf<T>;

		/// author id convert to account id and account id convert to author id
		type AuthorIdOf: Convert<Self::AccountId, Option<Self::ValidatorId>>;

		/// 
		type AccountIdOf: Convert<Self::AuthorIdOf, Self::AccountId>;

		/// a author is registered
		type AuthorRegistration: ValidatorRegistration<Self::ValidatorId>;
	}

	#[pallet::storage]
	#[pallet::getter(fn invulnerables)]
	/// the fixed collators that can not be kicked out.
	pub type Invulnerables<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn candidacy_bond)]
	/// the minimum bond to become a candidate
	pub type CandidacyBond<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn candidacy_bond)]
	/// the minimum staking amount to become the candidate
	pub type CollatorBond<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn candidate_info)]
	/// Get collator candidate info associated with an account if account is candidate else None
	pub(crate) type CandidatePool<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		Candidate<BalanceOf<T>, T::MaxDelegatorsOfCandidate>,
		OptionQuery,
	>;

	/// Top candidates staking
	#[pallet::storage]
	#[pallet::getter(fn top_candidates)]
	pub(crate) type TopCandidates<T: Config> = StorageValue<
		_,
		OrderedSet<Bond<T::AccountId, BalanceOf<T>>, T::MaxTopCandidates>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn delegators)]
	/// Get delegator associated with an collator candidate account if account is delegating else None
	pub(crate) type Delegators<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		Delegator<T::AccountId, BalanceOf<T>, T::MaxDelegatorsOfCandidate>,
		OptionQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn round)]
	/// the round(session) length (compute by blockNumbers)
	pub type Round<T: Config> = StorageValue<_, RoundInfo<T::BlockNumber>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total)]
	/// Total capital locked by this staking pallet
	pub(crate) type TotalStaking<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;


	#[pallet::storage]
	#[pallet::getter(fn total_collator_stake)]
	pub(crate) type TotalCollatorStake<T: Config> = StorageValue<_, TotalStake<BalanceOf<T>>, ValueQuery>;	

	#[pallet::storage]
	#[pallet::getter(fn selected_candidates)]
	/// The candidates number selected every round
	/// this can be modify by root_key, the top N
	pub type CollatorNums<T: Config> = StorageValue<_, u32, ValueQuery>;


	#[pallet::error]
	pub enum Error<T> {
		// Not the authorId
		NoAssociatedAuthorId,
		// Not registered the aouthor
		AuthorNotRegistered,
		// Not register the collator candidate
		CandidateNotRegistered,
		// current account Already become Candidate
		AlreadyCandidate,
		// already become vulnerables
		AlreadyInvulnerable,
		// below the minimum selected collator candidate numbers
		BelowMinSelectedCandidates,
		// set the same selected candidate numbers
		NoSettingSameCollatorNums,
		// below the minimum block length
		BelowMinlength,
		// set the same round length
		NotSameRoundLength,
		// round length less than the collator numbers per round
		RoundLengthLessThanCollatorNums,
		// has become candidate, but apply again
		CandidateExists,
		// has become delegators, but apply again or apply the candidate
		DelegatorExists,
		// no enough staking amount(below the CollatorBond) to apply for the candidate
		NotEnoughStakingForCandidate,
		// no sufficient balance to stake
		InsufficientBalance,
		// the candidate is not exist in candidate pool (which never apply for the candidate)
		CandidateNotExist,
		// already become a delegator, and apply for the delegator again
		AlreadyDelegate,
		// delegations is below the minimum the once delegation
		TooLowDelegation,
		// if a delegators' status is inactive, you can not delegate
		InactiveDelegator,
		// the delegations number exceed the maximum delegetions of a delegator
		TooManydelegations,
		// the bond of delegator is below the minimum amount
		TooLowDelegatorBond,
		// delegator exist in this candidate info
		DelegatorExistOfCandidate,
		// current candidate is inactive(eg: leaving)
		InactiveCandidate,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		// new invulnerables collator account list
		NewInvulnerables(Vec<T::AccountId>),
		// set the new bond value to become a candidate
		NewCandidacyBond(Balance<T>),
		// set the new round length
		RoundLengthSet(RoundIndex),
		// set the collator numbers(old numbers and new numbers)
		NewCollatorNumsSet(u32, u32),
		// add a new candidate
		NewCandidate(T::AccountId, BalanceOf<T>),
		// total candidate number
		TotalCandidate(u32),
		// total staking in this pallet
		TotalStaking(BalanceOf<T>),
		// new delegation of delegator
		NewDelegation(T::Account, T::Account, BalanceOf<T>),
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(n: T::BlockNumber) -> Weight {
			let weight = 0u32;
			let mut round = <Round<T>>::get();
			// check for round update(if now + first > length of Roud, we need to Update the Round)
			if round.should_update(now) {
				// 1. update round
				round.update(now);
				// start next round
				<Round<T>>::put(round);
				Self::deposit_event(Event::NewRound(round.first, round.current));
			}
			//
			weight
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// set fix collators by sudo account or by democracy(in stage 1)
		#[pallet::weight(0)]
		pub fn set_invulnerables(
			origin: OriginFor<T>,
			new: Vec<T::AccountId>,
		) -> DispatchResultWithPostInfo {
			// operated by sudo account
			ensure_root(origin)?;
			// we trust origin calls, this is just a for more accurate benchmarking
			if (new.len() as u32) > T::MaxInvulnerables::get() {
				log::warn!(
					"invulnerables > T::MaxInvulnerables; you might need to run benchmarks again"
				);
			}
			// check if the invulnerables have associated validator keys before they are set
			for account_id in &new {
				let authors_key = T::AuthorIdOf::convert(account_id.clone())
					.ok_or(Error::<T>::NoAssociatedAuthorId)?;
				ensure!(
					// ensure the key is in session list
					T::AuthorRegistration::is_registered(&authors_key),
					Error::<T>::CandidateNotRegistered
				);
			}
			// insert vulnerables collator
			<Invulnerables<T>>::put(&new);
			Self::deposit_event(Event::NewInvulnerables(new));
			Ok(().into())
		}

		/// Set the candidacy bond amount by sudo account
		#[pallet::weight(0)]
		pub fn set_candidacy_bond(
			origin: OriginFor<T>,
			bond: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			// operated by sudo account
			ensure_root(origin)?;
			<CandidacyBond<T>>::put(&bond);
			Self::deposit_event(Event::NewCandidacyBond(bond));
			Ok(().into())
		}

		/// Set the number of collator candidates selected per round
		#[pallet::weight(0)]
		pub fn set_collator_num(origin: OriginFor<T>, new: u32) -> DispatchResultWithPostInfo {
			// operated by sudo account
			ensure_root(origin)?;
			ensure!(new >= T::MinSelectedCandidates::get(), Error::<T>::BelowMinSelectedCandidates);
			let old = <CollatorNums<T>>::get();
			ensure!(old != new, Error::<T>::NoSettingSameCollatorNums);
			// ensure the round length more than collator nums
			ensure!(new <= <Round<T>>::get().length, Error::<T>::RoundLengthLessThanCollatorNums,);
			<CollatorNums<T>>::put(new);
			Self::deposit_event(Event::NewCollatorNumsSet(old, new));
			Ok(().into())
		}

		/// Set blocks per round
		/// if new round length is less than current round, will transition immediately
		/// in the next block
		#[pallet::weight(<T as Config>::WeightInfo::set_blocks_per_round())]
		pub fn set_round_length(
			origin: OriginFor<T>,
			new: T::BlockNumber,
		) -> DispatchResultWithPostInfo {
			// operated by sudo account
			ensure_root(origin)?;
			ensure!(new >= T::MinLengthPerRound::get(), Error::<T>::BelowMinlength);
			let mut round = <Round<T>>::get();
			let (round_index, round_first, old_length) = (round.current, round.first, round.length);
			// ensure the new length is not same as the old length
			ensure!(old_length != new, Error::<T>::NotSameRoundLength);
			// round length must more than the collator numbers per round
			ensure!(new >= <CollatorNums<T>>::get(), Error::<T>::RoundLengthLessThanCollatorNums,);
			round.length = new;
			<Round<T>>::put(round);
			Self::deposit_event(Event::RoundLengthSet(round_index, round_first, old_length, new));
			Ok(().into())
		}

		/// apply for becoming the candidate
		/// you need to stake some token to become a candidate which may be selected to become the collator
		#[pallet::weight(0)]
		pub fn apply_candidate(
			origin: OriginFor<T>,
			stake: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			// limit the account : ensure not the exist candidate, not the delegators, not the fix collators
			ensure!(!Self::is_active_candidate(&acc), Error::<T>::CandidateExists);
			ensure!(!Self::is_delegator(&acc), Error::<T>::DelegatorExists);
			ensure!(!Self::invulnerables().contains(&who), Error::<T>::AlreadyInvulnerable);
			// limit the stake: ensure the account has enough tokens to apply for the candidate
			ensure!(stake >= <CandidacyBond<T>>::get(), Error::<T>::NotEnoughStakingForCandidate);

			// stake balance
			T::Currency::reserve(&who, stake);
			// put into the candidate pool.
			let mut candidate = Candidate::new(stake);
			CandidatePool::insert(&who, &candidate);
			Self::deposit_event(Event::NewCandidate(&who, stake));

			// update the total staking aacount on Dora-KSM parachain(collator staking pallet)
			let new_total_staking = <TotalStaking<T>>::get().saturating_add(stake);
			<TotalStaking<T>>::put(new_total_staking);
			Self::deposit_event(Event::TotalStaking(new_total_staking));

			// update the top N candidates
			let n = Self::update_top_candidates(
				who,
				BalanceOf<T>::zero(),
				BalanceOf<T>::zero(),
				stake,
				BalanceOf<T>::zero(),
			);
			Self::deposit_event(Event::TotalCandidate(n));

			Ok(().into())
		}

		/// apply for the delegator to stake for the collator candidate
		/// the applicant should not be collator candidate
		/// if you never delegate a candidate, you need to use this function
		#[pallet::weights(0)]
		pub fn apply_delegate(
			origin: OriginFor<T>,
			candidate: T::AccountId,
			amount: BalanceOf<T>,
		) -> DispatchResultWithPostInfo {
			let who = ensure_signed(origin)?;
			// ensure the delegators has the bottom amount to delegate
			ensure!(T::Currency::can_reserve(&who, amount), Error::<T>::InsufficientBalance);
			let delegate_info = if let Some(mut dgt_state) = <Delegators<T>>::get(&who) {
				// if get the delegations, which means that is not the first delegation.
				// ensure the delegator is active
				ensure!(dgt_state.is_active(), Error::<T>::InactiveDelegator);
				// ensure every delegation amount more than the minimumDelagation
				ensure!(amount > T::MininumDelegation::get(), Error::<T>::TooLowDelegation);
				// ensure the delegations number less than the Max candidates can be delegate
				ensure!(
					(dgt_state.delegations.len().saturated_into::<u32>()) >=
						T::MaxCandidatesPerDelegator::get(),
					Error::<T>::TooManydelegations
				);
				// add a delegation to the delegator
				ensure!(
					dgt_state.add_delegation(Bond { owner: &candidate, amount }),
					Error::<T>::AlreadyDelegate,
				);
				dgt_state
			} else {
				// if the delegator state is none which means that is the first delegation.
				ensure!(amount > T::MinimumDelegatorBond::get(), Error::<T>::TooLowDelegatorBond);
				// ensure the comming delegator is not in the candidate pool
				// candidate and delegator can not be the same AccountId
				ensure!(Self::is_active_candidate(&who).is_none(), Error::<T>::CandidateExists);
				Delegator::new(&candidate, amount)
			};
			// ensure the candidate is in candidate pool and get the current candidate info
			let mut candidate_info =
				<CandidatePool<T>>::get(&candidate).ok_or(Error::<T>::CandidateNotExist)?;

			let delegations_nums: u32 = candidate_info.delegators.len().saturated_into();
			ensure!(!candidate_info.is_leaving(), Error::<T>::InactiveCandidate);

			let old_total = candidate_info.total_bond;
			let old_bond = candidate_info.bond;
			let delegation = Bond { owner: candidate, amount };
			// update the candidate's delegation info
			ensure!(
				// update the candidate's delegation info
				candidate_info.delegators.try_insert(delegation.clone()).unwrap_or(true),
				<Error<T>>::DelegatorExistOfCandidate
			);
			T::Currency::reserve(&who, amount);
			// update candidate and delegate state info
			<CandidatePool<T>>::insert(&candidate, candidate_info);
			let new_bond = <CandidatePool<T>>::get(&candidate).unwrap().total_bond;
			let new_total = <CandidatePool<T>>::get(&candidate).unwrap().total_total;
			<Delegators<T>>::insert(&who, delegate_info);
			Self::deposit_event(Event::NewDelegation(&who, &candidate, amount));
			let n = Self::update_top_candidates(
				candidate,
				old_bond,
				old_tatal - old_bond,
				new_bond,
				new_total,
			);
			Self::deposit_event(Event::TotalCandidate(n));
			Ok(().into())
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn account_id() -> T::AccountId {
			PALLET_ID.into_account()
		}

		/// Check whether an account is currently a candidate
		pub fn is_active_candidate(acc: &T::AccountId) -> bool {
			<CandidatePool<T>>::get(acc).is_some()
		}

		/// check whether an account is currently a delegator
		pub fn is_delegator(acc: &T::AccountId) -> bool {
			<Delegators<T>>::get(acc).is_some()
		}

		/// update candidate's bond
		fn update_top_candidates(
			candidate: T::AccountId,
			old_self: BalanceOf<T>,
			old_delagations: BalanceOf<T>,
			new_self: BalanceOf<T>,
			new_delagations: BalanceOf<T>,
		) -> u32 {
			// get the top numbers 
			let mut top_candidates = TopCandidates::<T>::get();
			let num_top_candidates: u32 = top_candidates.len().saturated_into();
			// get the old stake and new stake
			let old_stake =
				Bond { owner: candidate.clone(), amount: old_self.saturating_add(old_delagations) };
			let new_stake =
				Bond { owner: candidate.clone(), amount: new_self.saturating_add(new_delagations) };
			// update TopCandidates set
			let maybe_top_candidate_update = if let Ok(i) = top_candidates.linear_search(&old_stake)
			{
				// case 1: candidate is member of TopCandidates with old stake
				top_candidates.mutate(|vec| {
					if let Some(stake) = vec.get_mut(i) {
						stake.amount = new_stake.amount;
					}
				});
				// return the old stake candidate's index and top candidates
				Some((Some(i), top_candidates))
			} else if top_candidates.try_insert_replace(new_stake.clone()).is_ok() {
				// case 2: candidate ascends into TopCandidates with new stake
				// and might replace another candidate if TopCandidates is full
				Self::deposit_event(Event::EnteredTopCandidates(candidate));
				Some((None, top_candidates))
			} else {
				// case 3: candidate neither was nor will be member of TopCandidates
				None
			};

			// update storage for TotalCollatorStake and TopCandidates
			if let Some((maybe_old_idx, top_candidates)) = maybe_top_candidate_update {
				let max_selected_candidates =
					CollatorNums::<T>::get().saturated_into::<usize>();
				let was_collating =
					maybe_old_idx.map(|i| i < max_selected_candidates).unwrap_or(false);
				let is_collating = top_candidates
					.linear_search(&new_stake)
					.map(|i| i < max_selected_candidates)
					.unwrap_or(false);

				// update TopCollatorStake storage if candidate was or will be a collator
				match (was_collating, is_collating) {
					(true, true) => {
						Self::update_total_stake_by(
							new_self,
							new_delagations,
							old_self,
							old_delagations,
						);
					},
					(true, false) => {
						// candidate left the collator set because they staked less and have been
						// replaced by the next candidate in the queue at position
						// min(max_selected_candidates, top_candidates) - 1 in TopCandidates
						let new_col_idx =
							max_selected_candidates.min(top_candidates.len()).saturating_sub(1);

						// get displacer
						let (add_collators, add_delegators) =
							Self::get_top_candidate_stake_at(&top_candidates, new_col_idx)
								// shouldn't be possible to fail, but we handle it gracefully
								.unwrap_or((new_self, new_delagations));
						Self::update_total_stake_by(
							add_collators,
							add_delegators,
							old_self,
							old_delagations,
						);
					},
					(false, true) => {
						// candidate pushed out the least staked collator which is now at position
						// min(max_selected_top_candidates, top_candidates - 1) in TopCandidates
						let old_col_idx =
							max_selected_candidates.min(top_candidates.len().saturating_sub(1));

						// get amount to subtract from TotalCollatorStake
						let (drop_self, drop_delegators) =
							Self::get_top_candidate_stake_at(&top_candidates, old_col_idx)
								// default to zero if candidate DNE, e.g. TopCandidates is not full
								.unwrap_or((BalanceOf::<T>::zero(), BalanceOf::<T>::zero()));
						Self::update_total_stake_by(
							new_self,
							new_delagations,
							drop_self,
							drop_delegators,
						);
					},
					_ => {},
				}

				// update TopCandidates storage
				TopCandidates::<T>::put(top_candidates);
			}
			num_top_candidates
		}

		/// return top N candidates
		pub fn selected_top_candidates() -> BoundedVec<T::AccountId, T::MaxTopCandidates> {
			let candidates = TopCandidates::<T>::get();
			// get the top N number
			let top_n = CollatorNums::<T>::get().saturated_into::<usize>();
			log::trace!("{} Candidates for {} Collator seats", candidates.len(), top_n);
			// Choose the top collator Candidates qualified candidates
			let collators = candidates
				.into_iter()
				.take(top_n)
				.filter(|x| x.amount >= T::MinumunCollatorStake::get())
				.map(|x| x.owner)
				.collect::<Vec<T::AccountId>>();
			collators.try_into().expect("Did not extend Collators q.e.d.")
		}

		fn get_top_candidate_stake_at(
			top_candidates: &OrderedSet<BondOf<T>, T::MaxTopCandidates>,
			index: usize,
		) -> Option<(BalanceOf<T>, BalanceOf<T>)> {
			top_candidates
				.get(index)
				.and_then(|stake| CandidatePool::<T>::get(&stake.owner))
				// SAFETY: the total is always more than the stake
				// (bond, delagation)
				.map(|state| (state.stake, state.total - state.stake))
		}

		fn update_total_stake_by(
			add_collators: BalanceOf<T>,
			add_delegators: BalanceOf<T>,
			sub_collators: BalanceOf<T>,
			sub_delegators: BalanceOf<T>,
		) {
			TotalCollatorStake::<T>::mutate(|total| {
				total.collators = total
					.collators
					.saturating_sub(sub_collators)
					.saturating_add(add_collators);
				total.delegators = total
					.delegators
					.saturating_sub(sub_delegators)
					.saturating_add(add_delegators);
			});
		}

		/// Assemble the current set of top candidates and invulnerables into the next collator set.
		pub fn assemble_collators(candidates: Vec<T::AccountId>) -> Vec<T::AccountId> {
			let mut collators = Self::invulnerables();
			collators.extend(candidates.into_iter().collect::<Vec<_>>());
			collators
		}
	}

	/// Keep track of number of authored blocks per authority, uncles are counted as well since
	/// they're a valid proof of being online.
	/// Also, this is the proof for us to disribute reward to the block's author.
	impl<T: Config + pallet_authorship::Config + pallet_session::Config>
		pallet_authorship::EventHandler<T::AccountId, T::BlockNumber> for Pallet<T>
	{
		fn note_author(author: T::AccountId) {
			let pot = Self::account_id();
			// assumes an ED will be sent to pot.
			// distribute reward to collators
			// get the current collator's state info
			if let Some(candidate_state) = <CandidatePool<T>>::get(&author) {
				// Reward collator
				let total_stake = candidate_state.total_stake;
				// eg: set a block reward is 0.2 DORA
				let col_reward = (candidate_state.bond / total) * Perquintill::from_percent(20);
				T::Currency::deposit_into_existing(&author, col_reward);
				// reward delegators
				for Bond {owner, amount} in candidate_state.delegators{
					let del_reward = (amount / total) * Perquintill::from_percent(20);
					T::Currency::deposit_into_existing(&owner, del_reward);
				}
			}
			frame_system::Pallet::<T>::register_extra_weight_unchecked(
				T::WeightInfo::note_author(),
				DispatchClass::Mandatory,
			);
		}

		fn note_uncle(_author: T::AccountId, _age: T::BlockNumber) {
			//currently, don't care
		}
	}

	/// Play the role of the session manager. Mianly use `new_session`
	/// - assemble the collators(fix collators + active collators)
	/// - Aura can get the Authorities from the pallet-session, and author block round-robin-basis
	impl<T: Config> SessionManager<T::AccountId> for Pallet<T> {
		fn new_session(index: RoundIndex) -> Option<Vec<T::AccountId>> {
			log::info!(
				"assembling new collators for new session {} at #{:?}",
				index,
				<frame_system::Pallet<T>>::block_number(),
			);
			// get the collator selected from the active candidate pool
			let selected_collators = Pallet::<T>::selected_top_candidates().to_vec();
			//assemble collators = (vulnerables + active collators)
			let result = Self::assemble_collators(selected_candidates);
			frame_system::Pallet::<T>::register_extra_weight_unchecked(
				T::WeightInfo::new_session(candidates_len_before as u32, removed as u32),
				DispatchClass::Mandatory,
			);
			Some(result)
		}
		fn start_session(_: RoundIndex) {
			// we don't care.
		}
		fn end_session(_: RoundIndex) {
			// we don't care.
		}
	}
}
