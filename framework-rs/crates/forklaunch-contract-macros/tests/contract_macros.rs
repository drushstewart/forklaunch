use forklaunch_contract::ContractSchema;
use forklaunch_contract_macros::{ForklaunchContract, assert_route_params, route_operation_name};

#[derive(ForklaunchContract)]
struct CreateUserRequest;

#[test]
fn derives_contract_schema_ref() {
    let schema = CreateUserRequest::schema_ref();

    assert_eq!(schema.name, "CreateUserRequest");
}

#[test]
fn normalizes_operation_names_at_compile_time() {
    assert_eq!(route_operation_name!("Create User"), "createUser");
}

#[test]
fn validates_route_params_at_compile_time() {
    assert_route_params!("/users/:id/posts/{postId}" => ["id", "postId"]);
}
