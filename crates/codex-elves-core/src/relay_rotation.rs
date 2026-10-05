/**
 * @description 聚合供应商轮转选择器，负责按失败、对话、请求和权重策略选择已有中转配置。
 * @author Albert_Luo
 * @email 480199976@qq.com
 * @date 2026-05-27 00:00
 */
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::settings::{
    AggregateRelayProfile, AggregateRelayStrategy, BackendSettings, RelayProfile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    NoActiveAggregate,
    EmptyAggregateMembers {
        aggregate_id: String,
    },
    UnknownMemberRelay {
        aggregate_id: String,
        relay_id: String,
    },
    InvalidMemberRelay {
        aggregate_id: String,
        relay_id: String,
    },
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionError::NoActiveAggregate => write!(formatter, "未找到当前聚合供应商"),
            SelectionError::EmptyAggregateMembers { aggregate_id } => {
                write!(formatter, "聚合供应商「{aggregate_id}」没有成员")
            }
            SelectionError::UnknownMemberRelay {
                aggregate_id,
                relay_id,
            } => write!(
                formatter,
                "聚合供应商「{aggregate_id}」引用了不存在的供应商「{relay_id}」"
            ),
            SelectionError::InvalidMemberRelay {
                aggregate_id,
                relay_id,
            } => write!(
                formatter,
                "聚合供应商「{aggregate_id}」成员「{relay_id}」缺少 API Base URL 或 Key"
            ),
        }
    }
}

impl std::error::Error for SelectionError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RotationContext {
    pub conversation_id: Option<String>,
}

impl RotationContext {
    pub fn for_conversation(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: Some(conversation_id.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationEvent {
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub struct RelayRotationSelector {
    aggregate: AggregateRelayProfile,
    failover_index: usize,
    request_index: usize,
    weighted_member_index: usize,
    weighted_requests_served: u32,
    conversation_assignments: HashMap<String, String>,
}

static GLOBAL_SELECTOR: OnceLock<Mutex<Option<RelayRotationSelector>>> = OnceLock::new();

impl RelayRotationSelector {
    pub fn from_settings(settings: &BackendSettings) -> Result<Self, SelectionError> {
        let aggregate = active_aggregate(settings)?.clone();
        validate_aggregate_members(settings, &aggregate)?;
        Ok(Self {
            aggregate,
            failover_index: 0,
            request_index: 0,
            weighted_member_index: 0,
            weighted_requests_served: 0,
            conversation_assignments: HashMap::new(),
        })
    }

    pub fn select(
        &mut self,
        settings: &BackendSettings,
        context: RotationContext,
    ) -> Result<RelayProfile, SelectionError> {
        validate_aggregate_members(settings, &self.aggregate)?;
        let relay_id = match self.aggregate.strategy {
            AggregateRelayStrategy::Failover => self.member_id_at(self.failover_index),
            AggregateRelayStrategy::ConversationRoundRobin => {
                self.select_for_conversation(context.conversation_id)
            }
            AggregateRelayStrategy::RequestRoundRobin => self.select_next_request(),
            AggregateRelayStrategy::WeightedRoundRobin => self.select_next_weighted(),
        };
        relay_profile_by_id(settings, &relay_id).ok_or_else(|| SelectionError::UnknownMemberRelay {
            aggregate_id: self.aggregate.id.clone(),
            relay_id,
        })
    }

    pub fn peek(&self, settings: &BackendSettings) -> Result<RelayProfile, SelectionError> {
        validate_aggregate_members(settings, &self.aggregate)?;
        let relay_id = match self.aggregate.strategy {
            AggregateRelayStrategy::Failover => self.member_id_at(self.failover_index),
            AggregateRelayStrategy::ConversationRoundRobin
            | AggregateRelayStrategy::RequestRoundRobin => self.member_id_at(self.request_index),
            AggregateRelayStrategy::WeightedRoundRobin => {
                self.member_id_at(self.weighted_member_index)
            }
        };
        relay_profile_by_id(settings, &relay_id).ok_or_else(|| SelectionError::UnknownMemberRelay {
            aggregate_id: self.aggregate.id.clone(),
            relay_id,
        })
    }

    pub fn record_event(&mut self, event: RotationEvent) {
        if event == RotationEvent::Failure
            && self.aggregate.strategy == AggregateRelayStrategy::Failover
            && !self.aggregate.members.is_empty()
        {
            self.failover_index = (self.failover_index + 1) % self.aggregate.members.len();
        }
    }

    fn select_for_conversation(&mut self, conversation_id: Option<String>) -> String {
        let Some(conversation_id) = conversation_id else {
            return self.select_next_request();
        };
        if let Some(relay_id) = self.conversation_assignments.get(&conversation_id) {
            return relay_id.clone();
        }

        let relay_id = self.select_next_request();
        self.conversation_assignments
            .insert(conversation_id, relay_id.clone());
        relay_id
    }

    fn select_next_request(&mut self) -> String {
        let relay_id = self.member_id_at(self.request_index);
        self.request_index = (self.request_index + 1) % self.aggregate.members.len();
        relay_id
    }

    fn select_next_weighted(&mut self) -> String {
        let member = &self.aggregate.members[self.weighted_member_index];
        let relay_id = member.relay_id.clone();
        self.weighted_requests_served += 1;
        if self.weighted_requests_served >= member.weight.max(1) {
            self.weighted_requests_served = 0;
            self.weighted_member_index =
                (self.weighted_member_index + 1) % self.aggregate.members.len();
        }
        relay_id
    }

    fn member_id_at(&self, index: usize) -> String {
        self.aggregate.members[index % self.aggregate.members.len()]
            .relay_id
            .clone()
    }
}

pub fn select_relay_for_request(
    settings: &BackendSettings,
    context: RotationContext,
) -> Result<RelayProfile, SelectionError> {
    let Some(active_aggregate) = settings.active_aggregate_relay_profile() else {
        clear_global_selector();
        return Ok(settings.active_relay_profile());
    };

    let lock = GLOBAL_SELECTOR.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let needs_new_selector = guard
        .as_ref()
        .map(|selector| selector.aggregate != active_aggregate)
        .unwrap_or(true);
    if needs_new_selector {
        *guard = Some(RelayRotationSelector::from_settings(settings)?);
    }
    guard
        .as_mut()
        .expect("selector initialized")
        .select(settings, context)
}

pub fn select_relay_for_probe(settings: &BackendSettings) -> Result<RelayProfile, SelectionError> {
    let Some(active_aggregate) = settings.active_aggregate_relay_profile() else {
        clear_global_selector();
        return Ok(settings.active_relay_profile());
    };

    let lock = GLOBAL_SELECTOR.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let needs_new_selector = guard
        .as_ref()
        .map(|selector| selector.aggregate != active_aggregate)
        .unwrap_or(true);
    if needs_new_selector {
        *guard = Some(RelayRotationSelector::from_settings(settings)?);
    }
    guard.as_ref().expect("selector initialized").peek(settings)
}

pub fn fallback_relays_after(
    settings: &BackendSettings,
    relay_id: &str,
) -> Result<Vec<RelayProfile>, SelectionError> {
    let Some(active_aggregate) = settings.active_aggregate_relay_profile() else {
        return Ok(Vec::new());
    };
    validate_aggregate_members(settings, &active_aggregate)?;
    let start_index = active_aggregate
        .members
        .iter()
        .position(|member| member.relay_id == relay_id)
        .map(|index| index + 1)
        .unwrap_or(0);
    (0..active_aggregate.members.len().saturating_sub(1))
        .map(|offset| {
            let index = (start_index + offset) % active_aggregate.members.len();
            &active_aggregate.members[index]
        })
        .map(|member| {
            relay_profile_by_id(settings, &member.relay_id).ok_or_else(|| {
                SelectionError::UnknownMemberRelay {
                    aggregate_id: active_aggregate.id.clone(),
                    relay_id: member.relay_id.clone(),
                }
            })
        })
        .collect()
}

pub fn record_relay_request_event(settings: &BackendSettings, event: RotationEvent) {
    let Some(active_aggregate) = settings.active_aggregate_relay_profile() else {
        return;
    };
    let lock = GLOBAL_SELECTOR.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(selector) = guard
        .as_mut()
        .filter(|selector| selector.aggregate == active_aggregate)
    {
        selector.record_event(event);
    }
}

pub fn record_relay_request_failure(settings: &BackendSettings) {
    record_relay_request_event(settings, RotationEvent::Failure);
}

fn active_aggregate(settings: &BackendSettings) -> Result<&AggregateRelayProfile, SelectionError> {
    let active_id = settings
        .active_aggregate_relay_profile()
        .map(|aggregate| aggregate.id)
        .ok_or(SelectionError::NoActiveAggregate)?;

    settings
        .aggregate_relay_profiles
        .iter()
        .find(|aggregate| aggregate.id == active_id)
        .ok_or(SelectionError::NoActiveAggregate)
}

fn validate_aggregate_members(
    settings: &BackendSettings,
    aggregate: &AggregateRelayProfile,
) -> Result<(), SelectionError> {
    if aggregate.members.is_empty() {
        return Err(SelectionError::EmptyAggregateMembers {
            aggregate_id: aggregate.id.clone(),
        });
    }

    let relay_by_id = settings
        .relay_profiles
        .iter()
        .map(|profile| (profile.id.as_str(), profile))
        .collect::<HashMap<_, _>>();
    for member in &aggregate.members {
        let Some(relay) = relay_by_id.get(member.relay_id.as_str()) else {
            return Err(SelectionError::UnknownMemberRelay {
                aggregate_id: aggregate.id.clone(),
                relay_id: member.relay_id.clone(),
            });
        };
        if relay.base_url.trim().is_empty() || relay.api_key.trim().is_empty() {
            return Err(SelectionError::InvalidMemberRelay {
                aggregate_id: aggregate.id.clone(),
                relay_id: member.relay_id.clone(),
            });
        }
    }
    Ok(())
}

fn clear_global_selector() {
    let lock = GLOBAL_SELECTOR.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

fn relay_profile_by_id(settings: &BackendSettings, relay_id: &str) -> Option<RelayProfile> {
    settings
        .relay_profiles
        .iter()
        .find(|profile| profile.id == relay_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AggregateRelayMember;

    #[test]
    fn maximum_weight_boundary_advances_without_overflow() {
        let mut selector = RelayRotationSelector {
            aggregate: AggregateRelayProfile {
                id: "weighted-boundary".to_string(),
                name: String::new(),
                strategy: AggregateRelayStrategy::WeightedRoundRobin,
                members: vec![
                    AggregateRelayMember {
                        relay_id: "a".to_string(),
                        weight: u32::MAX,
                    },
                    AggregateRelayMember {
                        relay_id: "b".to_string(),
                        weight: 1,
                    },
                ],
            },
            failover_index: 0,
            request_index: 0,
            weighted_member_index: 0,
            weighted_requests_served: u32::MAX - 1,
            conversation_assignments: HashMap::new(),
        };
        assert_eq!(selector.select_next_weighted(), "a");
        assert_eq!(selector.select_next_weighted(), "b");
        assert_eq!(selector.select_next_weighted(), "a");
        assert_eq!(selector.weighted_requests_served, 1);
    }
}
