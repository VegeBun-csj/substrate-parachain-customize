// Copyright 2020-2021 Parity Technologies (UK) Ltd.
// This file is part of Cumulus.

// Cumulus is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Cumulus is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// You should have received a copy of the GNU General Public License
// along with Cumulus.  If not, see <http://www.gnu.org/licenses/>.

//! Pallet to spam the XCM/UMP.

#![cfg_attr(not(feature = "std"), no_std)]

use cumulus_pallet_xcm::{ensure_sibling_para, Origin as CumulusOrigin};
use cumulus_primitives_core::ParaId;
use frame_system::Config as SystemConfig;
use sp_runtime::traits::Saturating;
use sp_std::prelude::*;
use xcm::latest::{Error as XcmError, Junction, OriginKind, SendXcm, Xcm};

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// The module configuration trait.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// 确保能够接收普通交易也能够进行xcm的跨链消息
		type Origin: From<<Self as SystemConfig>::Origin>
			+ Into<Result<CumulusOrigin, <Self as Config>::Origin>>;

		/// The overarching call type; we assume sibling chains use the same type.
		type Call: From<Call<Self>> + Encode;

		type XcmSender: SendXcm;
	}

	/// 要Ping的目标链
	/// Targets是一个vec数组 =>  <parachainId：u8, payload：vec<u8>>
	#[pallet::storage]
	pub(super) type Targets<T: Config> = StorageValue<_, Vec<(ParaId, Vec<u8>)>, ValueQuery>;

	/// ping的总次数
	#[pallet::storage]
	pub(super) type PingCount<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// 已经发送的ping的次数
	#[pallet::storage]
	pub(super) type Pings<T: Config> =
		StorageMap<_, Blake2_128Concat, u32, T::BlockNumber, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	#[pallet::metadata(T::BlockNumber = "BlockNumber")]
	pub enum Event<T: Config> {
		PingSent(ParaId, u32, Vec<u8>),
		Pinged(ParaId, u32, Vec<u8>),
		PongSent(ParaId, u32, Vec<u8>),
		Ponged(ParaId, u32, Vec<u8>, T::BlockNumber),
		ErrorSendingPing(XcmError, ParaId, u32, Vec<u8>),
		ErrorSendingPong(XcmError, ParaId, u32, Vec<u8>),
		UnknownPong(ParaId, u32, Vec<u8>),
	}

	#[pallet::error]
	pub enum Error<T> {}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		// 每当区块finalize的时候，调用该方法
		fn on_finalize(n: T::BlockNumber) {
			// Targets是一个vec数组 =>  <parachainId：u8, payload：vec<u8>>
			// 获取需要进行ping的目标链，
			for (para, payload) in Targets::<T>::get().into_iter() {
				// 获取当前Ping的次数seq，并+1
				let seq = PingCount::<T>::mutate(|seq| {
					*seq += 1;
					*seq
				});
				match T::XcmSender::send_xcm(
					// 构造detination parachain
					(1, Junction::Parachain(para.into())).into(),
					// 构造消息
					Xcm::Transact {
						// native是以本身的链发送
						origin_type: OriginKind::Native,
						require_weight_at_most: 1_000,
						// Transact方法调用的时候主要是需要构造call
						// call的构造：
						// 1.如果pallet是同一个pallet，那么可以通过自己的pallet的call进行调用(ping-pong例子)
						// 2.如果不是同一个Pallet调用，那么可以先构造一个结构体来指定call的method到底是哪一个(xserver-xclient)
						// 调用目标链的ping方法，将seq，以及当前需要像目标链传的payload作为参数
						// 这里就是同一个pallet调用，就是使用的pallet自己的call进行构造
						call: <T as Config>::Call::from(Call::<T>::ping(seq, payload.clone()))
							.encode()
							.into(),
					},
				) {
					Ok(()) => {
						// 如果调用成功就增加把 <ping的次数,相应的区块号>
						Pings::<T>::insert(seq, n);
						Self::deposit_event(Event::PingSent(para, seq, payload));
					},
					Err(e) => {
						Self::deposit_event(Event::ErrorSendingPing(e, para, seq, payload));
					},
				}
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn start(origin: OriginFor<T>, para: ParaId, payload: Vec<u8>) -> DispatchResult {
			ensure_root(origin)?;
			Targets::<T>::mutate(|t| t.push((para, payload)));
			// 调用start方法的时候，首先会吧paraId和消息传入，然后等当前区块finalize，此时会调用on_finalize方法：
			// 其中首先会遍历Target，此时只有一个Target，
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn start_many(
			origin: OriginFor<T>,
			para: ParaId,
			count: u32,
			payload: Vec<u8>,
		) -> DispatchResult {
			ensure_root(origin)?;
			for _ in 0..count {
				Targets::<T>::mutate(|t| t.push((para, payload.clone())));
			}
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn stop(origin: OriginFor<T>, para: ParaId) -> DispatchResult {
			ensure_root(origin)?;
			Targets::<T>::mutate(|t| {
				if let Some(p) = t.iter().position(|(p, _)| p == &para) {
					t.swap_remove(p);
				}
			});
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn stop_all(origin: OriginFor<T>, maybe_para: Option<ParaId>) -> DispatchResult {
			ensure_root(origin)?;
			if let Some(para) = maybe_para {
				Targets::<T>::mutate(|t| t.retain(|&(x, _)| x != para));
			} else {
				Targets::<T>::kill();
			}
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn ping(origin: OriginFor<T>, seq: u32, payload: Vec<u8>) -> DispatchResult {
			// Only accept pings from other chains.
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;

			Self::deposit_event(Event::Pinged(para, seq, payload.clone()));
			match T::XcmSender::send_xcm(
				(1, Junction::Parachain(para.into())).into(),
				Xcm::Transact {
					origin_type: OriginKind::Native,
					require_weight_at_most: 1_000,
					// 调用Pong方法
					call: <T as Config>::Call::from(Call::<T>::pong(seq, payload.clone()))
						.encode()
						.into(),
				},
			) {
				Ok(()) => Self::deposit_event(Event::PongSent(para, seq, payload)),
				Err(e) => Self::deposit_event(Event::ErrorSendingPong(e, para, seq, payload)),
			}
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn pong(origin: OriginFor<T>, seq: u32, payload: Vec<u8>) -> DispatchResult {
			// 确保是其他平行链发送过来的消息
			let para = ensure_sibling_para(<T as Config>::Origin::from(origin))?;

			if let Some(sent_at) = Pings::<T>::take(seq) {
				Self::deposit_event(Event::Ponged(
					para,
					seq,
					payload,
					frame_system::Pallet::<T>::block_number().saturating_sub(sent_at),
				));
			} else {
				// Pong received for a ping we apparently didn't send?!
				Self::deposit_event(Event::UnknownPong(para, seq, payload));
			}
			Ok(())
		}
	}
}
