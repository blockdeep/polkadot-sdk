// Copyright (C) Parity Technologies (UK) Ltd.
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

//! Bridge definitions used on BridgeHub with the Westend flavor.

use crate::{
	bridge_common_config::DeliveryRewardInBalance, weights, xcm_config::UniversalLocation,
	BridgeRococoMessages, PolkadotXcm, Runtime, RuntimeEvent, XcmOverBridgeHubRococo, XcmRouter,
};
use bp_messages::{
	source_chain::FromBridgedChainMessagesDeliveryProof,
	target_chain::FromBridgedChainMessagesProof, LaneId,
};
use bp_parachains::SingleParaStoredHeaderDataBuilder;
use bridge_runtime_common::{
	extensions::refund_relayer_extension::{
		ActualFeeRefund, RefundBridgedMessages, RefundSignedExtensionAdapter,
		RefundableMessagesLane,
	},
	messages_xcm_extension::{
		SenderAndLane, XcmAsPlainPayload, XcmBlobHauler, XcmBlobHaulerAdapter,
		XcmBlobMessageDispatch, XcmVersionOfDestAndRemoteBridge,
	},
};
use codec::Encode;
use frame_support::{
	parameter_types,
	traits::{ConstU32, PalletInfoAccess},
};
use xcm::{
	latest::prelude::*,
	prelude::{InteriorLocation, NetworkId},
};
use xcm_builder::BridgeBlobDispatcher;

parameter_types! {
	pub const RelayChainHeadersToKeep: u32 = 1024;
	pub const ParachainHeadsToKeep: u32 = 64;

	pub const RococoBridgeParachainPalletName: &'static str = "Paras";
	pub const MaxRococoParaHeadDataSize: u32 = bp_rococo::MAX_NESTED_PARACHAIN_HEAD_DATA_SIZE;

	pub BridgeWestendToRococoMessagesPalletInstance: InteriorLocation = [PalletInstance(<BridgeRococoMessages as PalletInfoAccess>::index() as u8)].into();
	pub RococoGlobalConsensusNetwork: NetworkId = NetworkId::Rococo;
	pub RococoGlobalConsensusNetworkLocation: Location = Location::new(
		2,
		[GlobalConsensus(RococoGlobalConsensusNetwork::get())]
	);
	// see the `FEE_BOOST_PER_RELAY_HEADER` constant get the meaning of this value
	pub PriorityBoostPerRelayHeader: u64 = 32_007_814_407_814;
	// see the `FEE_BOOST_PER_PARACHAIN_HEADER` constant get the meaning of this value
	pub PriorityBoostPerParachainHeader: u64 = 1_396_340_903_540_903;
	// see the `FEE_BOOST_PER_MESSAGE` constant to get the meaning of this value
	pub PriorityBoostPerMessage: u64 = 182_044_444_444_444;

	pub AssetHubWestendParaId: cumulus_primitives_core::ParaId = bp_asset_hub_westend::ASSET_HUB_WESTEND_PARACHAIN_ID.into();
	pub AssetHubRococoParaId: cumulus_primitives_core::ParaId = bp_asset_hub_rococo::ASSET_HUB_ROCOCO_PARACHAIN_ID.into();

	// Lanes
	pub ActiveOutboundLanesToBridgeHubRococo: &'static [bp_messages::LaneId] = &[XCM_LANE_FOR_ASSET_HUB_WESTEND_TO_ASSET_HUB_ROCOCO];
	pub const AssetHubWestendToAssetHubRococoMessagesLane: bp_messages::LaneId = XCM_LANE_FOR_ASSET_HUB_WESTEND_TO_ASSET_HUB_ROCOCO;
	pub FromAssetHubWestendToAssetHubRococoRoute: SenderAndLane = SenderAndLane::new(
		ParentThen([Parachain(AssetHubWestendParaId::get().into())].into()).into(),
		XCM_LANE_FOR_ASSET_HUB_WESTEND_TO_ASSET_HUB_ROCOCO,
	);
	pub ActiveLanes: alloc::vec::Vec<(SenderAndLane, (NetworkId, InteriorLocation))> = alloc::vec![
			(
				FromAssetHubWestendToAssetHubRococoRoute::get(),
				(RococoGlobalConsensusNetwork::get(), [Parachain(AssetHubRococoParaId::get().into())].into())
			)
	];

	pub CongestedMessage: Xcm<()> = build_congestion_message(true).into();
	pub UncongestedMessage: Xcm<()> = build_congestion_message(false).into();

	pub BridgeHubRococoLocation: Location = Location::new(
		2,
		[
			GlobalConsensus(RococoGlobalConsensusNetwork::get()),
			Parachain(<bp_bridge_hub_rococo::BridgeHubRococo as bp_runtime::Parachain>::PARACHAIN_ID)
		]
	);
}
pub const XCM_LANE_FOR_ASSET_HUB_WESTEND_TO_ASSET_HUB_ROCOCO: LaneId = LaneId([0, 0, 0, 2]);

fn build_congestion_message<Call>(is_congested: bool) -> alloc::vec::Vec<Instruction<Call>> {
	alloc::vec![
		UnpaidExecution { weight_limit: Unlimited, check_origin: None },
		Transact {
			origin_kind: OriginKind::Xcm,
			require_weight_at_most:
				bp_asset_hub_westend::XcmBridgeHubRouterTransactCallMaxWeight::get(),
			call: bp_asset_hub_westend::Call::ToRococoXcmRouter(
				bp_asset_hub_westend::XcmBridgeHubRouterCall::report_bridge_status {
					bridge_id: Default::default(),
					is_congested,
				}
			)
			.encode()
			.into(),
		}
	]
}

/// Proof of messages, coming from Rococo.
pub type FromRococoBridgeHubMessagesProof =
	FromBridgedChainMessagesProof<bp_bridge_hub_rococo::Hash>;
/// Messages delivery proof for Rococo Bridge Hub -> Westend Bridge Hub messages.
pub type ToRococoBridgeHubMessagesDeliveryProof =
	FromBridgedChainMessagesDeliveryProof<bp_bridge_hub_rococo::Hash>;

/// Dispatches received XCM messages from other bridge
type FromRococoMessageBlobDispatcher =
	BridgeBlobDispatcher<XcmRouter, UniversalLocation, BridgeWestendToRococoMessagesPalletInstance>;

/// Export XCM messages to be relayed to the other side
pub type ToBridgeHubRococoHaulBlobExporter = XcmOverBridgeHubRococo;

pub struct ToBridgeHubRococoXcmBlobHauler;
impl XcmBlobHauler for ToBridgeHubRococoXcmBlobHauler {
	type Runtime = Runtime;
	type MessagesInstance = WithBridgeHubRococoMessagesInstance;

	type ToSourceChainSender = XcmRouter;
	type CongestedMessage = CongestedMessage;
	type UncongestedMessage = UncongestedMessage;
}

/// On messages delivered callback.
type OnMessagesDelivered = XcmBlobHaulerAdapter<ToBridgeHubRococoXcmBlobHauler, ActiveLanes>;

/// Signed extension that refunds relayers that are delivering messages from the Rococo parachain.
pub type OnBridgeHubWestendRefundBridgeHubRococoMessages = RefundSignedExtensionAdapter<
	RefundBridgedMessages<
		Runtime,
		RefundableMessagesLane<
			WithBridgeHubRococoMessagesInstance,
			AssetHubWestendToAssetHubRococoMessagesLane,
		>,
		ActualFeeRefund<Runtime>,
		PriorityBoostPerMessage,
		StrOnBridgeHubWestendRefundBridgeHubRococoMessages,
	>,
>;
bp_runtime::generate_static_str_provider!(OnBridgeHubWestendRefundBridgeHubRococoMessages);

/// Add GRANDPA bridge pallet to track Rococo relay chain.
pub type BridgeGrandpaRococoInstance = pallet_bridge_grandpa::Instance1;
impl pallet_bridge_grandpa::Config<BridgeGrandpaRococoInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type BridgedChain = bp_rococo::Rococo;
	type MaxFreeHeadersPerBlock = ConstU32<4>;
	type FreeHeadersInterval = ConstU32<5>;
	type HeadersToKeep = RelayChainHeadersToKeep;
	type WeightInfo = weights::pallet_bridge_grandpa::WeightInfo<Runtime>;
}

/// Add parachain bridge pallet to track Rococo BridgeHub parachain
pub type BridgeParachainRococoInstance = pallet_bridge_parachains::Instance1;
impl pallet_bridge_parachains::Config<BridgeParachainRococoInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = weights::pallet_bridge_parachains::WeightInfo<Runtime>;
	type BridgesGrandpaPalletInstance = BridgeGrandpaRococoInstance;
	type ParasPalletName = RococoBridgeParachainPalletName;
	type ParaStoredHeaderDataBuilder =
		SingleParaStoredHeaderDataBuilder<bp_bridge_hub_rococo::BridgeHubRococo>;
	type HeadsToKeep = ParachainHeadsToKeep;
	type MaxParaHeadDataSize = MaxRococoParaHeadDataSize;
}

/// Add XCM messages support for BridgeHubWestend to support Westend->Rococo XCM messages
pub type WithBridgeHubRococoMessagesInstance = pallet_bridge_messages::Instance1;
impl pallet_bridge_messages::Config<WithBridgeHubRococoMessagesInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = weights::pallet_bridge_messages::WeightInfo<Runtime>;

	type ThisChain = bp_bridge_hub_westend::BridgeHubWestend;
	type BridgedChain = bp_bridge_hub_rococo::BridgeHubRococo;
	type BridgedHeaderChain = pallet_bridge_parachains::ParachainHeaders<
		Runtime,
		BridgeParachainRococoInstance,
		bp_bridge_hub_rococo::BridgeHubRococo,
	>;

	type ActiveOutboundLanes = ActiveOutboundLanesToBridgeHubRococo;

	type OutboundPayload = XcmAsPlainPayload;

	type InboundPayload = XcmAsPlainPayload;
	type DeliveryPayments = ();

	type DeliveryConfirmationPayments = pallet_bridge_relayers::DeliveryConfirmationPaymentsAdapter<
		Runtime,
		WithBridgeHubRococoMessagesInstance,
		DeliveryRewardInBalance,
	>;

	type MessageDispatch = XcmBlobMessageDispatch<
		FromRococoMessageBlobDispatcher,
		Self::WeightInfo,
		cumulus_pallet_xcmp_queue::bridging::OutXcmpChannelStatusProvider<
			AssetHubWestendParaId,
			Runtime,
		>,
	>;
	type OnMessagesDelivered = OnMessagesDelivered;
}

/// Add support for the export and dispatch of XCM programs.
pub type XcmOverBridgeHubRococoInstance = pallet_xcm_bridge_hub::Instance1;
impl pallet_xcm_bridge_hub::Config<XcmOverBridgeHubRococoInstance> for Runtime {
	type UniversalLocation = UniversalLocation;
	type BridgedNetwork = RococoGlobalConsensusNetworkLocation;
	type BridgeMessagesPalletInstance = WithBridgeHubRococoMessagesInstance;
	type MessageExportPrice = ();
	type DestinationVersion = XcmVersionOfDestAndRemoteBridge<PolkadotXcm, BridgeHubRococoLocation>;
	type Lanes = ActiveLanes;
	type LanesSupport = ToBridgeHubRococoXcmBlobHauler;
}

#[cfg(test)]
mod tests {
	use super::*;
	use bridge_runtime_common::{
		assert_complete_bridge_types,
		extensions::refund_relayer_extension::RefundableParachain,
		integrity::{
			assert_complete_with_parachain_bridge_constants, check_message_lane_weights,
			AssertChainConstants, AssertCompleteBridgeConstants,
		},
	};
	use parachains_common::Balance;
	use testnet_parachains_constants::westend;

	/// Every additional message in the message delivery transaction boosts its priority.
	/// So the priority of transaction with `N+1` messages is larger than priority of
	/// transaction with `N` messages by the `PriorityBoostPerMessage`.
	///
	/// Economically, it is an equivalent of adding tip to the transaction with `N` messages.
	/// The `FEE_BOOST_PER_MESSAGE` constant is the value of this tip.
	///
	/// We want this tip to be large enough (delivery transactions with more messages = less
	/// operational costs and a faster bridge), so this value should be significant.
	const FEE_BOOST_PER_MESSAGE: Balance = 2 * westend::currency::UNITS;

	// see `FEE_BOOST_PER_MESSAGE` comment
	const FEE_BOOST_PER_RELAY_HEADER: Balance = 2 * westend::currency::UNITS;
	// see `FEE_BOOST_PER_MESSAGE` comment
	const FEE_BOOST_PER_PARACHAIN_HEADER: Balance = 2 * westend::currency::UNITS;

	#[test]
	fn ensure_bridge_hub_westend_message_lane_weights_are_correct() {
		check_message_lane_weights::<
			bp_bridge_hub_westend::BridgeHubWestend,
			Runtime,
			WithBridgeHubRococoMessagesInstance,
		>(
			bp_bridge_hub_rococo::EXTRA_STORAGE_PROOF_SIZE,
			bp_bridge_hub_westend::MAX_UNREWARDED_RELAYERS_IN_CONFIRMATION_TX,
			bp_bridge_hub_westend::MAX_UNCONFIRMED_MESSAGES_IN_CONFIRMATION_TX,
			true,
		);
	}

	#[test]
	fn ensure_bridge_integrity() {
		assert_complete_bridge_types!(
			runtime: Runtime,
			with_bridged_chain_grandpa_instance: BridgeGrandpaRococoInstance,
			with_bridged_chain_messages_instance: WithBridgeHubRococoMessagesInstance,
			this_chain: bp_bridge_hub_westend::BridgeHubWestend,
			bridged_chain: bp_bridge_hub_rococo::BridgeHubRococo,
		);

		assert_complete_with_parachain_bridge_constants::<
			Runtime,
			BridgeGrandpaRococoInstance,
			WithBridgeHubRococoMessagesInstance,
			bp_rococo::Rococo,
		>(AssertCompleteBridgeConstants {
			this_chain_constants: AssertChainConstants {
				block_length: bp_bridge_hub_westend::BlockLength::get(),
				block_weights: bp_bridge_hub_westend::BlockWeightsForAsyncBacking::get(),
			},
		});

		bridge_runtime_common::extensions::priority_calculator::per_relay_header::ensure_priority_boost_is_sane::<
			Runtime,
			BridgeGrandpaRococoInstance,
			PriorityBoostPerRelayHeader,
		>(FEE_BOOST_PER_RELAY_HEADER);

		bridge_runtime_common::extensions::priority_calculator::per_parachain_header::ensure_priority_boost_is_sane::<
			Runtime,
			RefundableParachain<WithBridgeHubRococoMessagesInstance, bp_bridge_hub_rococo::BridgeHubRococo>,
			PriorityBoostPerParachainHeader,
		>(FEE_BOOST_PER_PARACHAIN_HEADER);

		bridge_runtime_common::extensions::priority_calculator::per_message::ensure_priority_boost_is_sane::<
			Runtime,
			WithBridgeHubRococoMessagesInstance,
			PriorityBoostPerMessage,
		>(FEE_BOOST_PER_MESSAGE);

		assert_eq!(
			BridgeWestendToRococoMessagesPalletInstance::get(),
			[PalletInstance(
				bp_bridge_hub_westend::WITH_BRIDGE_WESTEND_TO_ROCOCO_MESSAGES_PALLET_INDEX
			)]
		);
	}
}
