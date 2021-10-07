#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
    use frame_system::pallet_prelude::*;
    use xcm::v0::{ Junction, OriginKind, SendXcm, Xcm,};
    use cumulus_primitives_core::ParaId;

    #[derive(Encode, Decode, Clone, PartialEq, Eq, Default, RuntimeDebug)]
    pub struct XregisterCall<AccountId> {
        // 这里的call_index是一个长度为2的vec，其中第一个参数为pallet名，第二个参数为pallet的方法名
        // 也就是调用哪个pallet的哪个方法
        call_index: [u8; 2],
        // 下面两个参数是需要向xserver注册提供的<account,name>，作为Register这个storage来进行存储
        account: AccountId,
        name: Vec<u8>,
    }

    // 这是XregisterCall结构体的一个构造方法
    impl<AccountId> XregisterCall<AccountId> {
        pub fn new(pallet_index: u8, method_index: u8, account: AccountId, name: Vec<u8>,)
                   -> Self {
            XregisterCall {
                call_index: [pallet_index, method_index],
                account: account,
                name: name,
            }
        }
    }

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

        /// The XCM sender module.
        type XcmSender: SendXcm;

        /// Xregister server's parachain ID
        type XregisterServerParachainId: Get<ParaId>;

        /// Xregister Pallet ID in xregister server
        type XregisterPalletID: Get<u8>;

        /// Xregister Method ID in xregister server
        type XregisterMethodID: Get<u8>;

        /// Xregister maximum weight
        type XregisterWeightAtMost: Get<u64>;
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub(super) trait Store)]
    pub struct Pallet<T>(_);

    #[pallet::event]
    #[pallet::metadata(T::AccountId = "AccountId")]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Xregister(T::AccountId, Vec<u8>),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Error to send xcm to Xregister server
        XcmSendError,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    #[pallet::call]
    impl<T: Config> Pallet<T> {

        #[pallet::weight(0)]
        pub fn xregister(origin: OriginFor<T>, name: Vec<u8>) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;

            // compose the call with pallet id, method id and arguments
            let call = XregisterCall::<T::AccountId>::new(
                T::XregisterPalletID::get(),    // palletId
                T::XregisterMethodID::get(),    // pallet中的方法的Id
                who.clone(),
                name.clone());

            // build the xcm transact message
            let message = Xcm::Transact {
                origin_type: OriginKind::Native,
                require_weight_at_most: T::XregisterWeightAtMost::get(),
                call: call.encode().into() };

            // send the message to xregister server chain
            // 这里调用进行跨链调用交易的时候其实是一个层级关系:
            // 在当前的parachain上调用的时候，会先到relaychain，即parent父级，然后进入到目标链parachain,再进入到该链的pallet，然后是其中的method
            match T::XcmSender::send_xcm((Junction::Parent,
                                          Junction::Parachain(T::XregisterServerParachainId::get().into())).into(),
                                         // 这里的message是通过struct构造出来的一个call
                                         message) {
                Ok(()) => {
                    // emit the event if send successfully
                    Self::deposit_event(Event::Xregister(who, name));
                    Ok(().into())
                },
                Err(_) => Err(Error::<T>::XcmSendError.into()),
            }
        }
    }
}