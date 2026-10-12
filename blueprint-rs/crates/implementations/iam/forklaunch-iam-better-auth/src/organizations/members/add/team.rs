use super::{AuthTransaction, BlueprintResult, DateTime, Model, Utc, Value, json, text};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub(super) async fn add<T: AuthTransaction>(
    tx: &mut T,
    mut team: Value,
    user: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    let id = text(&team, "id")?.to_owned();
    let pair =
        serde_json::to_vec(&json!([id, user])).map_err(|_| super::super::super::internal())?;
    let key = URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, &pair).as_ref());
    let rows = tx.list(Model::TeamMember).await?;
    if rows
        .iter()
        .any(|row| row["membershipKey"] == key || row["teamId"] == id && row["userId"] == user)
    {
        return Ok(());
    }
    let actual = i64::try_from(rows.iter().filter(|row| row["teamId"] == id).count())
        .map_err(|_| super::super::super::internal())?;
    // The adapter synchronizes only undercounts; it deliberately retains an
    // existing overcount. A legacy existing pair does not change the counter.
    let stored = team["memberCount"].as_i64().unwrap_or(0);
    if stored < actual {
        team["updatedAt"] = json!(super::super::super::timestamp(now));
    }
    let count = stored.max(actual);
    team["memberCount"] = json!(count + 1);
    tx.insert(Model::TeamMember, json!({"id":uuid::Uuid::new_v4().to_string(),"teamId":id,"userId":user,"membershipKey":key,"createdAt":super::super::super::timestamp(now)})).await?;
    tx.replace(Model::Team, team).await
}
