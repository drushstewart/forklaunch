//! Tenant/application-scoped cache while awaited role creation is active.
use super::{BlueprintResult, Grants};
use crate::organizations::Authorization;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
/// Trusted sharing scope for organization permission snapshots. No Debug/Serde.
/// Built-in stores share automatically within their tenant/application identity.
/// Custom stores may forward this handle; never share it across trusted tenants.
#[derive(Clone, Default)]
pub struct OrganizationPermissionCache(Arc<Inner>);
#[derive(Default)]
struct Inner {
    slots: Mutex<BTreeMap<String, Weak<Slot>>>,
    #[cfg(feature = "postgres")]
    _pool: Option<sqlx::PgPool>,
}
pub(super) struct Snapshot {
    pub(super) config: Authorization,
    pub(super) grants: Arc<Grants>,
}
pub(crate) struct Slot(Mutex<Option<Arc<Snapshot>>>);
impl OrganizationPermissionCache {
    pub(crate) fn reserve(&self, organization: &str) -> BlueprintResult<Arc<Slot>> {
        let mut slots = self.0.slots.lock().map_err(|_| super::empty_error())?;
        slots.retain(|_, slot| slot.strong_count() > 0);
        if let Some(slot) = slots.get(organization).and_then(Weak::upgrade) {
            return Ok(slot);
        }
        if organization.len() > 4096 || slots.len() >= 1024 {
            return Err(super::empty_error());
        }
        let slot = Arc::new(Slot(Mutex::new(None)));
        slots.insert(organization.into(), Arc::downgrade(&slot));
        Ok(slot)
    }
    pub(super) fn publish(
        &self,
        organization: &str,
        config: &Authorization,
        grants: Arc<Grants>,
    ) -> BlueprintResult<()> {
        let slot = self
            .0
            .slots
            .lock()
            .map_err(|_| super::empty_error())?
            .get(organization)
            .and_then(Weak::upgrade);
        if let Some(slot) = slot {
            *slot.0.lock().map_err(|_| super::empty_error())? = Some(Arc::new(Snapshot {
                config: config.clone(),
                grants,
            }));
        }
        Ok(())
    }
    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: &sqlx::PgPool, tenant: &str) -> Self {
        type Registry = Mutex<BTreeMap<(usize, String), Weak<Inner>>>;
        static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
        // options() points into the immutable shared PoolInner. Retaining a pool
        // clone in Inner prevents address reuse while this scope is alive.
        let identity = std::ptr::from_ref(pool.options()) as usize;
        let mut registry = REGISTRY
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.retain(|_, scope| scope.strong_count() > 0);
        let key = (identity, tenant.to_owned());
        if let Some(scope) = registry.get(&key).and_then(Weak::upgrade) {
            return Self(scope);
        }
        let scope = Arc::new(Inner {
            slots: Mutex::default(),
            _pool: Some(pool.clone()),
        });
        registry.insert(key, Arc::downgrade(&scope));
        Self(scope)
    }
}
impl Slot {
    pub(super) fn snapshot(&self) -> BlueprintResult<Arc<Snapshot>> {
        self.0
            .lock()
            .map_err(|_| super::empty_error())?
            .clone()
            .ok_or_else(super::empty_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AuthStore;
    #[test]
    fn memory_scope_clone_isolation_and_active_slot_bounds() {
        let store = crate::store::memory::MemoryAuthStore::default();
        let cache = store.organization_permission_cache().unwrap();
        let cloned = store.clone().organization_permission_cache().unwrap();
        let first = cache.reserve("same").unwrap();
        assert!(Arc::ptr_eq(&first, &cloned.reserve("same").unwrap()));
        let foreign = crate::store::memory::MemoryAuthStore::default()
            .organization_permission_cache()
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &foreign.reserve("same").unwrap()));
        drop(first);
        let slots: Vec<_> = (0..1024)
            .map(|i| cache.reserve(&i.to_string()).unwrap())
            .collect();
        assert!(cache.reserve("overflow").is_err());
        drop(slots);
        assert!(cache.reserve("recovered").is_ok());
        assert!(cache.reserve(&"x".repeat(4097)).is_err());
        assert!(cache.0.slots.lock().unwrap().len() <= 1);
    }
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[allow(clippy::used_underscore_binding)] // Inspect the otherwise retention-only pool anchor.
    async fn postgres_scope_uses_stable_pool_identity_and_tenant() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/owned-cache-test")
            .unwrap();
        let first = OrganizationPermissionCache::postgres(&pool, "tenant-a");
        let slot = first.reserve("org").unwrap();
        let cloned = pool.clone();
        // Mutable connection options must not change the immutable pool identity.
        pool.set_connect_options((*pool.connect_options()).clone());
        let second = OrganizationPermissionCache::postgres(&cloned, "tenant-a");
        assert!(Arc::ptr_eq(&slot, &second.reserve("org").unwrap()));
        let foreign = OrganizationPermissionCache::postgres(&pool, "tenant-b");
        assert!(!Arc::ptr_eq(&slot, &foreign.reserve("org").unwrap()));
        let separate = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/owned-cache-test")
            .unwrap();
        assert!(!Arc::ptr_eq(
            &slot,
            &OrganizationPermissionCache::postgres(&separate, "tenant-a")
                .reserve("org")
                .unwrap()
        ));
        drop(pool);
        drop(cloned);
        assert!(first.0._pool.is_some());
        assert!(Arc::ptr_eq(&slot, &second.reserve("org").unwrap()));
    }
}
