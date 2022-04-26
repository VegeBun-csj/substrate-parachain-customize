use frame_support::{pallet_prelude::*, traits::ReservableCurrency};
use codec::{Decode, Encode};
use sp_runtime::{
	traits::{AtLeast32BitUnsigned, Saturating, Zero},
	Perbill, Percent, RuntimeDebug,
};
use sp_std::{cmp::Ordering, collections::btree_map::BTreeMap, prelude::*, fmt::Debug};
use crate::{RoundIndex, set::{OrderedSet}};

#[derive(Clone, Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct Bond<AccountId, Balance> {
	pub owner: AccountId,
	pub amount: Balance,
}

impl<A: Decode, B: Default> Default for Bond<A, B> {
	fn default() -> Bond<A, B> {
		Bond {
			owner: A::decode(&mut sp_runtime::traits::TrailingZeroInput::zeroes())
				.expect("infinite length input; no invalid inputs for type; qed"),
			amount: B::default(),
		}
	}
}

impl<A, B: Default> Bond<A, B> {
	pub fn from_owner(owner: A) -> Self {
		Bond {
			owner,
			amount: B::default(),
		}
	}
}

impl<AccountId: Ord, Balance> Eq for Bond<AccountId, Balance> {}

impl<AccountId: Ord, Balance> Ord for Bond<AccountId, Balance> {
	fn cmp(&self, other: &Self) -> Ordering {
		self.owner.cmp(&other.owner)
	}
}

impl<AccountId: Ord, Balance> PartialOrd for Bond<AccountId, Balance> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl<AccountId: Ord, Balance> PartialEq for Bond<AccountId, Balance> {
	fn eq(&self, other: &Self) -> bool {
		self.owner == other.owner
	}
}


#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum CandidateStatus {
	/// Committed to be online and producing valid blocks (not equivocating)
	Active,
	Leaving(RoundIndex),
}

impl Default for CandidateStatus {
	fn default() -> CandidateStatus {
		CandidateStatus::Active
	}
}

#[derive(Encode, Decode, Clone, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(MaxDelegatorsOfCandidate))]
#[codec(mel_bound(Balance: MaxEncodedLen))]
/// Global collator state with commission fee, Bondd amount, and delegations
pub struct Candidate<Balance, MaxDelegatorsOfCandidate>
where
	Balance: Eq + Ord + Debug,
	MaxDelegatorsOfCandidate: Get<u32> + Debug + PartialEq,
{
	/// The candidate's staking amount.
	pub bond: Balance,

	/// The delegators that back the candidate.
	pub delegators: OrderedSet<Bond<AccountId, Balance>, MaxDelegatorsOfCandidate>,

	/// The total backing a collator has.
	/// Should equal the sum of all delegators stake adding collators stake
	pub total_bond: Balance,

	/// The current status of the candidate. Indicates whether a candidate is
	/// active or leaving the candidate pool
	pub status: CandidateStatus,
}


impl<B, S> Candidate<B, S>
where
	B: AtLeast32BitUnsigned + Ord + Copy + Saturating + Debug + Zero,
	S: Get<u32> + Debug + PartialEq,
{
	pub fn new(stake: B) -> Self {
		// the first bond will be add to total_bond	
		let total_bond = stake;
		Candidate {
			bond,
			delegators: OrderedSet::new(),
			total_bond,
			status: CandidateStatus::default(), // default active
		}
	}

	pub fn is_active(&self) -> bool {
		self.status == CandidateStatus::Active
	}

	pub fn is_leaving(&self) -> bool {
		matches!(self.status, CandidateStatus::Leaving(_))
	}

	pub fn can_exit(&self, when: u32) -> bool {
		matches!(self.status, CandidateStatus::Leaving(at) if at <= when )
	}

	pub fn revert_leaving(&mut self) {
		self.status = CandidateStatus::Active;
	}

	pub fn stake_more(&mut self, more: B) {
		self.bond = self.bond.saturating_add(more);
		self.total_bond = self.total_bond.saturating_add(more);
	}

	// Returns None if underflow or less == self.stake (in which case collator
	// should leave).
	pub fn stake_less(&mut self, less: B) -> Option<B> {
		if self.bond > less {
			self.bond = self.bond.saturating_sub(less);
			self.total_bond = self.total_bond.saturating_sub(less);
			Some(self.bond)
		} else {
			None
		}
	}

	pub fn inc_delegator(&mut self, delegator: A, more: B) {
		if let Ok(i) = self.delegators.linear_search(&Bond::<A, B> {
			owner: delegator,
			amount: B::zero(),
		}) {
			self.delegators
				.mutate(|vec| vec[i].amount = vec[i].amount.saturating_add(more));
			self.total_bond = self.total_bond.saturating_add(more);
			self.delegators.sort_greatest_to_lowest()
		}
	}

	pub fn dec_delegator(&mut self, delegator: A, less: B) {
		if let Ok(i) = self.delegators.linear_search(&Bond::<A, B> {
			owner: delegator,
			amount: B::zero(),
		}) {
			self.delegators
				.mutate(|vec| vec[i].amount = vec[i].amount.saturating_sub(less));
			self.total_bond = self.total_bond.saturating_sub(less);
			self.delegators.sort_greatest_to_lowest()
		}
	}

	pub fn leave_candidates(&mut self, round: RoundIndex) {
		self.status = CandidateStatus::Leaving(round);
	}
}

#[derive(Clone, PartialEq, Encode, Decode, RuntimeDebug, TypeInfo)]
pub enum DelegatorStatus {
	/// Active with no scheduled exit
	Active,
	/// Schedule exit to revoke all ongoing delegations
	Leaving(RoundIndex),
}


#[derive(Encode, Decode, RuntimeDebug, PartialEq, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(MaxDelegatorsOfCandidate))]
#[codec(mel_bound(AccountId: MaxEncodedLen, Balance: MaxEncodedLen))]
pub struct Delegator<AccountId: Eq + Ord, Balance: Eq + Ord, MaxDelegatorsOfCandidate: Get<u32>> {
	pub delegations: OrderedSet<Bond<AccountId, Balance>, MaxDelegatorsOfCandidate>,
	pub total: Balance,
	pub status: DelegatorStatus,
}

impl<AccountId, Balance, MaxDelegatorsOfCandidate> Delegator<AccountId, Balance, MaxDelegatorsOfCandidate>
where
	AccountId: Eq + Ord + Clone + Debug,
	Balance: Copy + Add<Output = Balance> + Saturating + PartialOrd + Eq + Ord + Debug + Zero,
	MaxDelegatorsOfCandidate: Get<u32> + Debug + PartialEq,
{

	/// new a delegator
	pub fn new(collator_candidate: AccountId, amount: Balance) -> Result<Self, ()> {
		Ok(Delegator {
			delegations: OrderedSet::from(
				vec![Bond {
					owner: collator_candidate,
					amount,
				}]
				.try_into()?,
			),
			total: amount,
			status: DelegatorStatus::Active,
		})
	}

	/// 
	/// Is a active delegator? which is delegating some collator candidates
	pub fn is_active(&self) -> bool {
		matches!(self.status, DelegatorStatus::Active)
	}

	pub fn is_leaving(&self) -> bool {
		matches!(self.status, DelegatorStatus::Leaving(_))
	}

	/// Adds a new delegation.
	///
	/// If already delegating to the same account, this call returns false and
	/// doesn't insert the new delegation.
	pub fn add_delegation(&mut self, stake: Bond<AccountId, Balance>) -> bool {
		let amt = stake.amount;
		if self.delegations.try_insert(stake).map_err(|_| ())? {
			self.total = self.total.saturating_add(amt);
			true
		} else {
			false
		}
	}

	/// Returns Some(remaining stake for delegator) if the delegation for the
	/// collator exists. Returns `None` otherwise.
	pub fn rm_delegation(&mut self, collator: &AccountId) -> Option<Balance> {
		let amt = self.delegations.remove(&Bond::<AccountId, Balance> {
			owner: collator.clone(),
			// amount is irrelevant for removal
			amount: Balance::zero(),
		});

		if let Some(Bond::<AccountId, Balance> { amount: balance, .. }) = amt {
			self.total = self.total.saturating_sub(balance);
			Some(self.total)
		} else {
			None
		}
	}

	/// Returns None if delegation was not found.
	pub fn inc_delegation(&mut self, collator: AccountId, more: Balance) -> Option<Balance> {
		if let Ok(i) = self.delegations.linear_search(&Bond::<AccountId, Balance> {
			owner: collator,
			amount: Balance::zero(),
		}) {
			self.delegations
				.mutate(|vec| vec[i].amount = vec[i].amount.saturating_add(more));
			self.total = self.total.saturating_add(more);
			self.delegations.sort_greatest_to_lowest();
			Some(self.delegations[i].amount)
		} else {
			None
		}
	}

	/// Returns Some(Some(balance)) if successful, None if delegation was not
	/// found and Some(None) if delegated Bond would underflow.
	pub fn dec_delegation(&mut self, collator: AccountId, less: Balance) -> Option<Option<Balance>> {
		if let Ok(i) = self.delegations.linear_search(&Bond::<AccountId, Balance> {
			owner: collator,
			amount: Balance::zero(),
		}) {
			if self.delegations[i].amount > less {
				self.delegations
					.mutate(|vec| vec[i].amount = vec[i].amount.saturating_sub(less));
				self.total = self.total.saturating_sub(less);
				self.delegations.sort_greatest_to_lowest();
				Some(Some(self.delegations[i].amount))
			} else {
				// underflow error; should rm entire delegation
				Some(None)
			}
		} else {
			None
		}
	}
}



/// The Round contains: `Session` and `block` 
#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct RoundInfo<BlockNumber> {
	/// Current round index.
	pub current: RoundIndex,
	/// The first block of the current round.
	pub first: BlockNumber,
	/// The length of the current round in blocks.
	pub length: BlockNumber,
}

impl<B> RoundInfo<B>
where
	B: Copy + Saturating + From<u32> + PartialOrd,
{
	pub fn new(current: RoundIndex, first: B, length: B) -> RoundInfo<B> {
		RoundInfo { current, first, length }
	}

	/// Checks if the round should be updated.
	///
	/// The round should update if `self.length` or more blocks where produced
	/// after `self.first`.
	pub fn should_update(&self, now: B) -> bool {
		let l = now.saturating_sub(self.first);
		l >= self.length
	}

	/// Starts a new round.
	pub fn update(&mut self, now: B) {
		self.current = self.current.saturating_add(1u32);
		self.first = now;
	}
}

impl<B> Default for RoundInfo<B>
where
	B: Copy + Saturating + Add<Output = B> + Sub<Output = B> + From<u32> + PartialOrd,
{
	fn default() -> RoundInfo<B> {
        // by default: session start at 0, and the round(session) length is 30.
		RoundInfo::new(0u32, 0u32.into(), 30.into())
	}
}

/// The number of delegations a delegator has done within the last session in
/// which they delegated.
#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, TypeInfo, MaxEncodedLen)]
pub struct DelegationCounter {
	/// The index of the last delegation.
	pub round: SessionIndex,
	/// The number of delegations made within round.
	pub counter: u32,
}


/// The total stake of the pallet.
///
/// The stake includes both collators' and delegators' staked funds.
#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
pub struct TotalStake<Balance: Default> {
	pub collators: Balance,
	pub delegators: Balance,
}


pub type BondOf<T> = Bond<AccountIdOf<T>, BalanceOf<T>>;