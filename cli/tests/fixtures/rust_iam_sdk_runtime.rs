use iam_sdk::{IamClient,IamUserGetUserRequest,IamUserGetUserResponses,IamUserGetBatchUsersRequest,IamUserGetBatchUsersResponses,IamUserUpdateUserRequest,IamUserUpdateUserResponses,IamRoleGetRoleRequest,IamRoleGetRoleResponses};
use std::collections::BTreeMap;
fn headers() -> BTreeMap<String,String> {BTreeMap::from([("authorization".into(),std::env::var("IAM_JWT_HEADER").unwrap())])}
#[tokio::test]
async fn directory_crud_scoping_and_nested_models() {
    let client=IamClient::new(std::env::var("IAM_BASE_URL").unwrap());
    let user_id=std::env::var("IAM_USER_ID").unwrap();
    let outsider=std::env::var("IAM_OUTSIDER_ID").unwrap();
    let role_id=std::env::var("IAM_ROLE_ID").unwrap();
    let response=client.user_get_user(IamUserGetUserRequest{params:BTreeMap::from([("id".into(),user_id.clone())]),headers:headers(),..Default::default()}).await.unwrap();
    let IamUserGetUserResponses::Status200(user)=response else {panic!("get user failed")};
    assert_eq!(user.id,user_id);assert_eq!(user.roles[0].id,role_id);assert_eq!(user.roles[0].permissions.len(),1);
    let response=client.role_get_role(IamRoleGetRoleRequest{params:BTreeMap::from([("id".into(),role_id.clone())]),headers:headers(),..Default::default()}).await.unwrap();
    let IamRoleGetRoleResponses::Status200(role)=response else {panic!("get role failed")};assert_eq!(role.id,role_id);
    let response=client.user_get_batch_users(IamUserGetBatchUsersRequest{query:BTreeMap::from([("ids".into(),serde_json::to_string(&vec![user_id.clone(),outsider.clone()]).unwrap())]),headers:headers(),..Default::default()}).await.unwrap();
    let IamUserGetBatchUsersResponses::Status200(users)=response else {panic!("get batch users failed")};assert_eq!(users.len(),1);assert_eq!(users[0].id,user_id);
    let response=client.user_update_user(IamUserUpdateUserRequest{body:Some(serde_json::json!({"id":user_id,"firstName":"Native SDK"}).to_string()),headers:headers(),..Default::default()}).await.unwrap();
    assert!(matches!(response,IamUserUpdateUserResponses::Status200(_)));
    let response=client.user_update_user(IamUserUpdateUserRequest{body:Some(serde_json::json!({"id":outsider,"firstName":"Denied"}).to_string()),headers:headers(),..Default::default()}).await.unwrap();
    assert!(matches!(response,IamUserUpdateUserResponses::Status403(_)));
    let response=client.user_get_user(IamUserGetUserRequest{params:BTreeMap::from([("id".into(),user_id)]),..Default::default()}).await.unwrap();
    assert!(matches!(response,IamUserGetUserResponses::Status401(_)));
}
