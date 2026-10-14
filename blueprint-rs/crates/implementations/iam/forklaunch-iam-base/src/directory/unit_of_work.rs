//! Typed application mapping inside one tenant-bound directory transaction.
//!
//! Core schemas and mapper rules remain authoritative. Custom DTOs map into the
//! existing four entity families; this is not a custom SQL/entity-table API.
use super::{DirectoryKind, DirectoryTransaction, invalid, not_found, record_id, storage_error};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use forklaunch_iam_interface::services::{
    CreateOrganizationDto, CreatePermissionDto, CreateRoleDto, CreateUserDto, OrganizationDto,
    PermissionDto, RoleDto, UpdateOrganizationDto, UpdatePermissionDto, UpdateRoleDto,
    UpdateUserDto, UserDto,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

mod sealed {
    pub trait Sealed {}
}

/// A supported family and its core mapper DTOs. Sealed to preserve storage rules.
pub trait DirectoryEntity: sealed::Sealed + Send + Sync {
    const KIND: DirectoryKind;
    type Create: Serialize + Send;
    type Update: Serialize + Send;
    type Dto: DeserializeOwned + Send;
}
macro_rules! entity {
    ($name:ident, $kind:ident, $create:ty, $update:ty, $dto:ty) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $name;
        impl sealed::Sealed for $name {}
        impl DirectoryEntity for $name {
            const KIND: DirectoryKind = DirectoryKind::$kind;
            type Create = $create;
            type Update = $update;
            type Dto = $dto;
        }
    };
}
entity!(
    Permissions,
    Permission,
    CreatePermissionDto,
    UpdatePermissionDto,
    PermissionDto
);
entity!(Roles, Role, CreateRoleDto, UpdateRoleDto, RoleDto);
entity!(Users, User, CreateUserDto, UpdateUserDto, UserDto);
entity!(
    Organizations,
    Organization,
    CreateOrganizationDto,
    UpdateOrganizationDto,
    OrganizationDto
);

/// Application-owned typed inputs, projection and extra context. Hooks can use
/// this same unit of work for related directory operations; no raw connection,
/// tenant override, credentials or SQL interface is supplied.
pub trait DirectoryMapper<E: DirectoryEntity>: Sync {
    type Create: Send;
    type Update: Send;
    type Output: Send;
    type Context: Sync;
    fn create<T: DirectoryTransaction>(
        &self,
        input: Self::Create,
        work: &mut DirectoryUnitOfWork<T>,
        context: &Self::Context,
    ) -> impl Future<Output = BlueprintResult<E::Create>> + Send;
    fn update<T: DirectoryTransaction>(
        &self,
        input: Self::Update,
        work: &mut DirectoryUnitOfWork<T>,
        context: &Self::Context,
    ) -> impl Future<Output = BlueprintResult<E::Update>> + Send;
    fn project(
        &self,
        entity: E::Dto,
        context: &Self::Context,
    ) -> impl Future<Output = BlueprintResult<Self::Output>> + Send;
}

/// One atomic, audited directory graph operation. An error permanently poisons
/// the transaction, including errors an application catches inside mapper hooks.
/// Dropping without a successful commit rolls back storage and audit changes.
pub struct DirectoryUnitOfWork<T: DirectoryTransaction> {
    tx: T,
    organization_scope: Option<String>,
    failed: Arc<AtomicBool>,
}
impl<T: DirectoryTransaction> DirectoryUnitOfWork<T> {
    pub(super) fn new(tx: T, organization_scope: Option<String>) -> Self {
        Self {
            tx,
            organization_scope,
            failed: Arc::new(AtomicBool::new(false)),
        }
    }
    fn ready(&self) -> BlueprintResult<()> {
        if self.failed.load(Ordering::Acquire) {
            Err(poisoned())
        } else {
            Ok(())
        }
    }
    fn finish<R>(&mut self, result: BlueprintResult<R>) -> BlueprintResult<R> {
        if result.is_err() {
            self.failed.store(true, Ordering::Release);
        }
        result
    }
    fn authorize(&self, kind: DirectoryKind, row: &Value, mutation: bool) -> BlueprintResult<()> {
        if kind == DirectoryKind::User
            && let Some(scope) = &self.organization_scope
        {
            let Some(organization) = row["organization"].as_str() else {
                return Err(not_found());
            };
            if organization != scope {
                return Err(if mutation {
                    BlueprintError::new(
                        "iam_directory_forbidden",
                        "Forbidden: user belongs to another organization",
                    )
                } else {
                    not_found()
                });
            }
        }
        Ok(())
    }
    async fn read<E: DirectoryEntity>(&mut self, id: &str) -> BlueprintResult<E::Dto> {
        let row = self.tx.get(E::KIND, id).await?.ok_or_else(not_found)?;
        self.authorize(E::KIND, &row, false)?;
        serde_json::from_value(super::mapping::hydrate(&mut self.tx, E::KIND, row).await?)
            .map_err(|_| storage_error())
    }
    /// Creates through the standard mapper, including password ignoring and
    /// relationship checks. The returned value is not committed yet.
    /// # Errors
    /// Any validation, scope, storage or projection error poisons this transaction.
    pub async fn create<E: DirectoryEntity>(
        &mut self,
        input: E::Create,
    ) -> BlueprintResult<E::Dto> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let value = encode(input)?;
            self.authorize(E::KIND, &value, true)?;
            let row = super::mapping::create(&mut self.tx, E::KIND, value).await?;
            self.read::<E>(record_id(&row)?).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Updates using the existing core DTO and scope rules.
    /// # Errors
    /// A failed operation poisons this transaction.
    pub async fn update<E: DirectoryEntity>(
        &mut self,
        input: E::Update,
    ) -> BlueprintResult<E::Dto> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let value = encode(input)?;
            let id = record_id(&value)?;
            let row = self.tx.get(E::KIND, id).await?.ok_or_else(not_found)?;
            self.authorize(E::KIND, &row, true)?;
            let row = super::mapping::update(&mut self.tx, E::KIND, value).await?;
            self.read::<E>(record_id(&row)?).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Reads the current transaction's hydrated DTO, including staged writes.
    /// # Errors
    /// Missing/out-of-scope rows and storage failures poison this transaction.
    pub async fn get<E: DirectoryEntity>(&mut self, id: &str) -> BlueprintResult<E::Dto> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = self.read::<E>(id).await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Deletes a core directory row with ordinary relationship cleanup and
    /// explicit-identity restrictions. Does not delete credential accounts.
    /// # Errors
    /// Missing/out-of-scope rows, bindings or storage failures poison this transaction.
    pub async fn delete<E: DirectoryEntity>(&mut self, id: &str) -> BlueprintResult<()> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let row = self.tx.get(E::KIND, id).await?.ok_or_else(not_found)?;
            self.authorize(E::KIND, &row, true)?;
            self.tx.delete(E::KIND, id).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Applies custom create input/context and output mapping in this transaction.
    /// # Errors
    /// Hook and core operation failures poison the transaction, including caught failures.
    pub async fn mapped_create<E: DirectoryEntity, M: DirectoryMapper<E>>(
        &mut self,
        mapper: &M,
        input: M::Create,
        context: &M::Context,
    ) -> BlueprintResult<M::Output> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let core = mapper.create(input, self, context).await?;
            let row = self.create::<E>(core).await?;
            mapper.project(row, context).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Applies custom update input/context and output mapping in this transaction.
    /// # Errors
    /// Hook and core operation failures poison the transaction.
    pub async fn mapped_update<E: DirectoryEntity, M: DirectoryMapper<E>>(
        &mut self,
        mapper: &M,
        input: M::Update,
        context: &M::Context,
    ) -> BlueprintResult<M::Output> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let core = mapper.update(input, self, context).await?;
            let row = self.update::<E>(core).await?;
            mapper.project(row, context).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Projects a hydrated row into an application-specific output type.
    /// # Errors
    /// Read/projection errors poison this transaction.
    pub async fn mapped_get<E: DirectoryEntity, M: DirectoryMapper<E>>(
        &mut self,
        mapper: &M,
        id: &str,
        context: &M::Context,
    ) -> BlueprintResult<M::Output> {
        self.ready()?;
        let mut operation = Operation::new(self.failed.clone());
        let result = async {
            let row = self.get::<E>(id).await?;
            mapper.project(row, context).await
        }
        .await;
        operation.complete = result.is_ok();
        self.finish(result)
    }
    /// Atomically commits all staged graph and audit changes.
    /// # Errors
    /// Rejects a poisoned transaction; also propagates storage commit failures.
    pub async fn commit(self) -> BlueprintResult<()> {
        self.ready()?;
        self.tx.commit().await
    }
}
fn encode(input: impl Serialize) -> BlueprintResult<Value> {
    let value = serde_json::to_value(input).map_err(|_| invalid())?;
    if serde_json::to_vec(&value).map_err(|_| invalid())?.len() > 1024 * 1024 {
        return Err(invalid());
    }
    Ok(value)
}
fn poisoned() -> BlueprintError {
    BlueprintError::new(
        "iam_directory_transaction_failed",
        "Directory transaction cannot commit after a failed operation",
    )
}

#[cfg(test)]
pub(crate) mod tests;

// A canceled or panicking future may already have staged changes. Poisoning on
// drop prevents the caller from committing those changes after catching unwind
// or timing out a mapper hook. Nested operations share the same failure flag.
struct Operation {
    failed: Arc<AtomicBool>,
    complete: bool,
}
impl Operation {
    fn new(failed: Arc<AtomicBool>) -> Self {
        Self {
            failed,
            complete: false,
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.complete {
            self.failed.store(true, Ordering::Release);
        }
    }
}
