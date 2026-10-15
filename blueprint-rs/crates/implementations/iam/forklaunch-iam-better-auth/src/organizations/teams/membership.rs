use super::{AuthTransaction, BlueprintResult, DateTime, Model, Utc, Value, json, text};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub(in crate::organizations) async fn add<T: AuthTransaction>(
    tx: &mut T,
    mut team: Value,
    user: &str,
    now: DateTime<Utc>,
    limit: Option<u32>,
) -> BlueprintResult<Value> {
    let id = text(&team, "id")?.to_owned();
    let pair = serde_json::to_vec(&json!([id, user])).map_err(|_| super::super::internal())?;
    let key = URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref());
    if let Some(existing) = tx
        .find_page(Model::TeamMember, "membershipKey", &key, None, 1)
        .await?
        .records
        .into_iter()
        .next()
    {
        return Ok(existing);
    }
    let rows = crate::store::find_all(tx, Model::TeamMember, "teamId", &id).await?;
    if let Some(existing) = rows.iter().find(|row| row["userId"] == user) {
        return Ok(existing.clone());
    }
    let actual = i64::try_from(rows.iter().filter(|row| row["teamId"] == id).count())
        .map_err(|_| super::super::internal())?;
    // The adapter synchronizes only undercounts; it deliberately retains an
    // existing overcount. A legacy existing pair does not change the counter.
    let stored = team["memberCount"].as_i64().unwrap_or(0);
    if stored < actual {
        team["updatedAt"] = json!(super::super::timestamp(now));
    }
    let count = stored.max(actual);
    if limit.is_some_and(|limit| count >= i64::from(limit)) {
        return Err(super::super::team_policy::member_limit());
    }
    team["memberCount"] = json!(count.checked_add(1).ok_or_else(super::super::internal)?);
    let membership = json!({"id":uuid::Uuid::new_v4().to_string(),"teamId":id,"userId":user,"membershipKey":key,"createdAt":super::super::timestamp(now)});
    tx.insert(Model::TeamMember, membership.clone()).await?;
    tx.replace(Model::Team, team).await?;
    Ok(membership)
}
