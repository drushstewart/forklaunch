//! Admission is repeated after a separately committed reinvite cancellation.
use super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Model, OrganizationService, Utc, Value,
    allowed, error, input, json, missing, text, unauthorized,
};
pub(super) struct Admission {
    pub org: String,
    pub email: String,
    pub role: String,
    pub actor: String,
}
impl<S: AuthStore> OrganizationService<S> {
    pub(super) async fn invitation_admission<T: AuthTransaction>(
        &self,
        tx: &mut T,
        token: &str,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Admission> {
        let view = self
            .recheck(tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = body["organizationId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
            })
            .ok_or_else(missing)?;
        let email = text(body, "email")?.to_lowercase();
        crate::accounts::validate_email(&email)?;
        let member = crate::store::find_all(tx, Model::Member, "organizationId", org)
            .await?
            .into_iter()
            .find(|row| row["organizationId"] == org && row["userId"] == view.user["id"])
            .ok_or_else(|| error("MEMBER_NOT_FOUND", "Member not found"))?;
        if !allowed(
            tx,
            org,
            text(&member, "role")?,
            Some(&json!({"invitation":["create"]})),
        )
        .await?
        {
            return Err(error(
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION",
                "You are not allowed to invite users to this organization",
            ));
        }
        let role = input::roles(&body["role"]);
        input::validate_roles(tx, org, &role).await?;
        if !text(&member, "role")?
            .split(',')
            .any(|role| input::trim(role) == "owner")
            && role.split(',').any(|role| role == "owner")
        {
            return Err(error(
                "YOU_ARE_NOT_ALLOWED_TO_INVITE_USER_WITH_THIS_ROLE",
                "You are not allowed to invite a user with this role",
            ));
        }
        input::ensure_not_member(tx, org, &email).await?;
        Ok(Admission {
            org: org.to_owned(),
            email,
            role,
            actor: text(&view.user, "id")?.to_owned(),
        })
    }
}
