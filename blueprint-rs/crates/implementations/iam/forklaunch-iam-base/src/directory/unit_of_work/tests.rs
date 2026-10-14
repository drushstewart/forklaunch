use super::*;
use crate::directory::{DirectoryStore, memory::MemoryDirectoryStore, service::BaseIamDirectory};
use serde_json::json;
use std::sync::Arc;

pub(crate) struct Mapper;
pub(crate) struct Context {
    pub organization: String,
    pub prefix: String,
    pub reject: bool,
}
#[derive(Debug, PartialEq)]
pub(crate) struct Profile {
    pub id: String,
    pub display: String,
    pub grant: String,
}
impl DirectoryMapper<Users> for Mapper {
    type Create = String;
    type Update = (String, String);
    type Output = Profile;
    type Context = Context;
    async fn create<T: DirectoryTransaction>(
        &self,
        email: String,
        work: &mut DirectoryUnitOfWork<T>,
        context: &Context,
    ) -> BlueprintResult<CreateUserDto> {
        let organization = work.get::<Organizations>(&context.organization).await?;
        let permission = work
            .create::<Permissions>(
                serde_json::from_value(json!({"slug":format!("{}:read",context.prefix)})).unwrap(),
            )
            .await?;
        let role = work
            .create::<Roles>(
                serde_json::from_value(json!({"name":"Custom","permissionIds":[permission.id]}))
                    .unwrap(),
            )
            .await?;
        Ok(serde_json::from_value(json!({"email":email,"password":"NEVER-STORED","firstName":"Custom","lastName":"Member","organization":organization.id,"roles":[role.id],"providerFields":{"applicationLabel":context.prefix}})).unwrap())
    }
    async fn update<T: DirectoryTransaction>(
        &self,
        input: (String, String),
        work: &mut DirectoryUnitOfWork<T>,
        _: &Context,
    ) -> BlueprintResult<UpdateUserDto> {
        let row = work.get::<Users>(&input.0).await?;
        Ok(serde_json::from_value(
            json!({"id":row.id,"firstName":input.1,"password":"ALSO-IGNORED"}),
        )
        .unwrap())
    }
    async fn project(&self, entity: UserDto, context: &Context) -> BlueprintResult<Profile> {
        assert_ne!(context.prefix, "panic", "owned mapper panic");
        if context.reject {
            return Err(invalid());
        }
        Ok(Profile {
            id: entity.id,
            display: format!("{}:{}", context.prefix, entity.first_name),
            grant: entity.roles[0].permissions[0].slug.clone(),
        })
    }
}
pub(crate) fn organization(label: &str) -> CreateOrganizationDto {
    serde_json::from_value(json!({"name":label,"domain":"fixture.test","subscription":label}))
        .unwrap()
}

/// Reused against the production `PostgreSQL` store with its restricted role.
pub(crate) async fn mapped_graph<S: DirectoryStore + 'static>(
    service: &BaseIamDirectory<S>,
) -> String {
    let mut work = service.unit_of_work().await.unwrap();
    let org = work
        .create::<Organizations>(organization("mapped-org"))
        .await
        .unwrap();
    let context = Context {
        organization: org.id,
        prefix: "custom".into(),
        reject: false,
    };
    let profile = work
        .mapped_create::<Users, _>(&Mapper, "custom@example.test".into(), &context)
        .await
        .unwrap();
    assert_eq!(profile.display, "custom:Custom");
    assert_eq!(profile.grant, "custom:read");
    let updated = work
        .mapped_update::<Users, _>(&Mapper, (profile.id.clone(), "Updated".into()), &context)
        .await
        .unwrap();
    assert_eq!(updated.display, "custom:Updated");
    let projected = work
        .mapped_get::<Users, _>(&Mapper, &profile.id, &context)
        .await
        .unwrap();
    assert_eq!(updated, projected);
    work.commit().await.unwrap();
    profile.id
}

#[tokio::test]
async fn mapper_composes_typed_graph_and_custom_context() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    let user = mapped_graph(&service).await;
    let mut tx = store.begin(false).await.unwrap();
    let row = tx.get(DirectoryKind::User, &user).await.unwrap().unwrap();
    assert!(row.get("password").is_none());
    assert_eq!(row["providerFields"]["applicationLabel"], "custom");
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn failed_projection_and_caught_core_error_poison_all_staged_writes() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    let mut work = service.unit_of_work().await.unwrap();
    let org = work
        .create::<Organizations>(organization("rollback-org"))
        .await
        .unwrap();
    let context = Context {
        organization: org.id.clone(),
        prefix: "rollback".into(),
        reject: true,
    };
    assert!(
        work.mapped_create::<Users, _>(&Mapper, "rollback@example.test".into(), &context)
            .await
            .is_err()
    );
    assert!(work.commit().await.is_err());
    let mut tx = store.begin(false).await.unwrap();
    for kind in [
        DirectoryKind::User,
        DirectoryKind::Role,
        DirectoryKind::Permission,
        DirectoryKind::Organization,
    ] {
        assert!(tx.list(kind, None, None).await.unwrap().is_empty());
    }
    tx.commit().await.unwrap();
    let mut work = service.unit_of_work().await.unwrap();
    work.create::<Organizations>(organization("caught-error"))
        .await
        .unwrap();
    assert!(work.get::<Users>("missing").await.is_err());
    assert!(
        work.create::<Organizations>(organization("must-not-run"))
            .await
            .is_err()
    );
    assert!(work.commit().await.is_err());
}

#[tokio::test]
async fn scope_cannot_be_changed_by_custom_mapper_or_core_crud() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    let user = mapped_graph(&service).await;
    let scoped =
        BaseIamDirectory::new(store.clone()).with_organization_scope("different-org".into());
    let mut work = scoped.unit_of_work().await.unwrap();
    assert!(work.get::<Users>(&user).await.is_err());
    assert!(work.commit().await.is_err());
    let mut work = scoped.unit_of_work().await.unwrap();
    assert!(work.delete::<Users>(&user).await.is_err());
    assert!(work.commit().await.is_err());
    let mut work = scoped.unit_of_work().await.unwrap();
    assert!(
        work.update::<Users>(
            serde_json::from_value(json!({"id":user,"firstName":"Denied"})).unwrap()
        )
        .await
        .is_err()
    );
    assert!(work.commit().await.is_err());
    let mut work = scoped.unit_of_work().await.unwrap();
    let dto=serde_json::from_value(json!({"email":"scope@example.test","password":"ignored","firstName":"Scope","lastName":"Test","roles":[],"organization":"another-org"})).unwrap();
    assert!(work.create::<Users>(dto).await.is_err());
    assert!(work.commit().await.is_err());
}

struct InterruptedMapper;
impl DirectoryMapper<Organizations> for InterruptedMapper {
    type Create = ();
    type Update = ();
    type Output = OrganizationDto;
    type Context = ();
    async fn create<T: DirectoryTransaction>(
        &self,
        (): (),
        work: &mut DirectoryUnitOfWork<T>,
        (): &(),
    ) -> BlueprintResult<CreateOrganizationDto> {
        work.create::<Organizations>(organization("interrupted-staged"))
            .await?;
        std::future::pending().await
    }
    async fn update<T: DirectoryTransaction>(
        &self,
        (): (),
        _: &mut DirectoryUnitOfWork<T>,
        (): &(),
    ) -> BlueprintResult<UpdateOrganizationDto> {
        Err(invalid())
    }
    async fn project(&self, value: OrganizationDto, (): &()) -> BlueprintResult<OrganizationDto> {
        Ok(value)
    }
}
#[tokio::test]
async fn canceled_mapper_future_poison_prevents_partial_commit() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    let mut work = service.unit_of_work().await.unwrap();
    // Poll once: the in-memory graph write completes, then the hook is pending.
    let mut future = Box::pin(work.mapped_create::<Organizations, _>(&InterruptedMapper, (), &()));
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    assert!(future.as_mut().poll(&mut context).is_pending());
    drop(future);
    assert!(work.commit().await.is_err());
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.list(DirectoryKind::Organization, None, None)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn caught_mapper_panic_prevents_commit_and_invalid_email_is_rejected() {
    let store = Arc::new(MemoryDirectoryStore::default());
    let service = BaseIamDirectory::new(store.clone());
    let user = mapped_graph(&service).await;
    let mut work = service.unit_of_work().await.unwrap();
    let organization = work
        .create::<Organizations>(organization("panic-staged"))
        .await
        .unwrap();
    let context = Context {
        organization: organization.id.clone(),
        prefix: "panic".into(),
        reject: false,
    };
    let mut future = Box::pin(work.mapped_get::<Users, _>(&Mapper, &user, &context));
    let waker = std::task::Waker::noop();
    let mut poll_context = std::task::Context::from_waker(waker);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future
            .as_mut()
            .poll(&mut poll_context)))
        .is_err()
    );
    drop(future);
    assert!(work.commit().await.is_err());
    let mut read = store.begin(false).await.unwrap();
    assert!(
        read.get(DirectoryKind::Organization, &organization.id)
            .await
            .unwrap()
            .is_none()
    );
    read.commit().await.unwrap();
    let mut work = service.unit_of_work().await.unwrap();
    let dto=serde_json::from_value(json!({"email":"invalid","password":"ignored","firstName":"Invalid","lastName":"Email","roles":[]})).unwrap();
    assert!(work.create::<Users>(dto).await.is_err());
    assert!(work.commit().await.is_err());
}
