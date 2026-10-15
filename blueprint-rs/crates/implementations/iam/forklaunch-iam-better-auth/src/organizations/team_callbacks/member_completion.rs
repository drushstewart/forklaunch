use super::{AutocommitAdapter, TeamMemberLimitPolicy, member, seat};
use crate::{
    organizations::{OrganizationService, text},
    sessions::SessionView,
    store::{AuthStore, AuthTransaction},
};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
impl<S: AuthStore> OrganizationService<S> {
    pub(in crate::organizations) async fn complete_member_with_team_callback(
        &self,
        row: Value,
        team: &str,
        view: Option<&SessionView>,
        now: DateTime<Utc>,
        hook: &dyn TeamMemberLimitPolicy,
    ) -> BlueprintResult<Value> {
        let result = async {
            let view = view.ok_or_else(super::super::unauthorized)?;
            let mut adapter = AutocommitAdapter(self.store.as_ref());
            let limit = member(
                hook,
                text(&row, "organizationId")?,
                team,
                view,
                &mut adapter,
            )
            .await?;
            let mut tx = self.store.begin(true).await?;
            seat(
                &mut tx,
                team,
                text(&row, "userId")?,
                self.time_source.sample(now),
                limit,
                self.team_seat_query_profile,
            )
            .await?;
            tx.commit().await
        }
        .await;
        if let Err(error) = result {
            let mut tx = self.store.begin(true).await?;
            super::super::members::delete_member(&mut tx, &row).await?;
            tx.commit().await?;
            return Err(error);
        }
        Ok(row)
    }
}
