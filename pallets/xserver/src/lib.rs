#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;

	use cumulus_primitives_core::ParaId;
	use cumulus_pallet_xcm::{Origin as CumulusOrigin, ensure_sibling_para};

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		// origin could from both extrinsic and xcm message
		type Origin: From<<Self as frame_system::Config>::Origin> + Into<Result<CumulusOrigin, <Self as Config>::Origin>>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn register)]
	pub type Register<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, Vec<u8>, ValueQuery>;

	#[pallet::event]
	#[pallet::metadata(T::AccountId = "AccountId")]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		// Xregister event include source parachain id, account and its name
		Xregister(ParaId, T::AccountId, Vec<u8>),
	}

	#[pallet::error]
	pub enum Error<T> {
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn xregister(origin: OriginFor<T>, account: T::AccountId, name: Vec<u8>) -> DispatchResultWithPostInfo {
			// get the source parachain id from origin
			let para_id = ensure_sibling_para(<T as Config>::Origin::from(origin))?;

			// insert account with its name
			Register::<T>::insert(account.clone(), name.clone());

			// emit event
			Self::deposit_event(Event::Xregister(para_id, account, name));

			Ok(().into())
		}
	}
}