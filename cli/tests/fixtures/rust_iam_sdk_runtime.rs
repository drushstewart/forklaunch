use std::collections::BTreeMap;

use iam_sdk::{
    Client, GetBatchUsersQuery, GetBatchUsersRequest, GetBatchUsersResponses,
    GetRoleParams, GetRoleRequest, GetRoleResponses, GetUserParams, GetUserRequest,
    GetUserResponses, UpdateUserRequest, UpdateUserResponses,
};
fn headers() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "authorization".into(),
        std::env::var("IAM_JWT_HEADER").unwrap(),
    )])
}
#[tokio::test]
async fn directory_crud_scoping_and_nested_models() {
    let client = Client::new(std::env::var("IAM_BASE_URL").unwrap())
        .with_plaintext_development_transport();
    let user_id = std::env::var("IAM_USER_ID").unwrap();
    let outsider = std::env::var("IAM_OUTSIDER_ID").unwrap();
    let role_id = std::env::var("IAM_ROLE_ID").unwrap();
    let response = client
        .get_user(GetUserRequest {
            params: GetUserParams {
                id: user_id.clone(),
            },
            headers: headers(),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetUserResponses::Status200(user) = response else {
        panic!("get user failed")
    };
    assert_eq!(user.id, user_id);
    assert_eq!(user.roles[0].id, role_id);
    assert_eq!(user.roles[0].permissions.len(), 1);
    let response = client
        .get_role(GetRoleRequest {
            params: GetRoleParams {
                id: role_id.clone(),
            },
            headers: headers(),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetRoleResponses::Status200(role) = response else {
        panic!("get role failed")
    };
    assert_eq!(role.id, role_id);
    let response = client
        .get_batch_users(GetBatchUsersRequest {
            query: GetBatchUsersQuery {
                ids: vec![user_id.clone(), outsider.clone()],
            },
            headers: headers(),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetBatchUsersResponses::Status200(users) = response else {
        panic!("get batch users failed")
    };
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, user_id);
    let response = client
        .update_user(UpdateUserRequest {
            body: Some(
                serde_json::from_value(serde_json::json!({"id":user_id,"firstName":"Native SDK"}))
                    .unwrap(),
            ),
            headers: headers(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(response, UpdateUserResponses::Status200(_)));
    let response = client
        .update_user(UpdateUserRequest {
            body: Some(
                serde_json::from_value(serde_json::json!({"id":outsider,"firstName":"Denied"}))
                    .unwrap(),
            ),
            headers: headers(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(response, UpdateUserResponses::Status403(_)));
    let response = client
        .get_user(GetUserRequest {
            params: GetUserParams { id: user_id },
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(response, GetUserResponses::Status401(_)));
}
