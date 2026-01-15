//
// Copyright (c) 2026 ZettaScale Technology
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// http://www.eclipse.org/legal/epl-2.0, or the Apache License, Version 2.0
// which is available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//
// Contributors:
//   ZettaScale Zenoh Team, <zenoh@zettascale.tech>
//
use zenoh_config::{
    gateway::{GatewayFiltersConf, GatewayPresetConf, GatewaySouthConf},
    ExpandedConfig, Interface, WhatAmI,
};
#[allow(unused_imports)]
use zenoh_core::polyfill::*;
use zenoh_protocol::core::{Bound, Region};
use zenoh_result::ZResult;
use zenoh_transport::TransportPeer;

/// Computes the _transient_ [`Region`] of a remote.
///
/// This method is used during the Open phase of establishment to decide whether a remote is
/// south-bound using the [`zenoh_protocol::transport::open::ext::South`] extension.
#[tracing::instrument(level = "debug", skip(config), ret)]
pub(crate) fn compute_transient_region_of(
    peer: &TransportPeer,
    config: &ExpandedConfig,
) -> ZResult<Region> {
    const ROUTER_REGION_LIMITATION_ERROR: &str =
        "Router regions cannot be subregions of non-router regions (unsupported)";

    // FIXME(regions): this is misleading, we only deicide whether to send the South extension.
    match config.gateway.south.clone().unwrap_or_default() {
        GatewaySouthConf::Preset(GatewayPresetConf::Auto) => {
            let region = match (config.mode(), peer.whatami) {
                (WhatAmI::Router, WhatAmI::Peer | WhatAmI::Client) => Region::South {
                    id: Default::default(),
                    mode: peer.whatami,
                },
                (WhatAmI::Peer, WhatAmI::Client) => Region::South {
                    id: Default::default(),
                    mode: peer.whatami,
                },
                _ => Region::North,
            };

            Ok(region)
        }
        GatewaySouthConf::Custom(subregions) => {
            if let Some(id) = subregions
                .iter()
                .position(|s| is_match(s.filters.as_deref(), peer))
            {
                if peer.whatami.is_router() && !config.mode().is_router() {
                    bail!("{}", ROUTER_REGION_LIMITATION_ERROR)
                }

                Ok(Region::South {
                    id,
                    mode: peer.whatami,
                })
            } else {
                Ok(Region::North)
            }
        }
    }
}

/// Computes the [`Region`] of a remote.
///
/// ## Invariants
///
/// 1. If [`compute_region_of`] succeeds, then it will return the same [`transient_region`] computed
///   during establishment.
#[tracing::instrument(level = "debug", skip(config), ret)]
pub(crate) fn compute_region_of(
    peer: &TransportPeer,
    config: &ExpandedConfig,
    transient_region: &Region,
    remote_bound: &Bound,
) -> ZResult<Region> {
    match (transient_region.bound(), remote_bound) {
        (Bound::South, Bound::North) | (Bound::North, Bound::South) => Ok(*transient_region),
        (Bound::South, Bound::South) => {
            bail!("South-south bound configuration (invalid)")
        }
        (Bound::North, Bound::North) => {
            if peer.whatami != config.mode() {
                bail!("North-north bound configuration with different modes (invalid)")
            }

            Ok(*transient_region)
        }
    }
}

#[allow(clippy::incompatible_msrv)]
fn is_match(filter: Option<&[GatewayFiltersConf]>, peer: &TransportPeer) -> bool {
    filter.is_none_or(|filters| {
        filters.iter().any(|filter| {
            let value = filter
                .zids
                .as_ref()
                .is_none_or(|zid| zid.contains(&peer.zid.into()))
                && filter.interfaces.as_ref().is_none_or(|ifaces| {
                    peer.links
                        .iter()
                        .flat_map(|link| {
                            link.interfaces
                                .iter()
                                .map(|iface| Interface(iface.to_owned()))
                        })
                        .all(|iface| ifaces.contains(&iface))
                })
                && filter
                    .modes
                    .as_ref()
                    .is_none_or(|mode| mode.matches(peer.whatami))
                && filter.region_names.as_ref().is_none_or(|region_names| {
                    peer.region_name
                        .as_ref()
                        .is_some_and(|region_name| region_names.iter().any(|n| n == region_name))
                });

            if filter.negated {
                !value
            } else {
                value
            }
        })
    })
}
