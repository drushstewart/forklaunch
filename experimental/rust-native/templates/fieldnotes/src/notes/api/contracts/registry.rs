//! Ordered HTTP operations composed from domain schemas and route policy.
use framework_core::contract::{ContractRegistry, ServiceContract};

pub fn registry() -> ContractRegistry {
    let mut registry = ContractRegistry::for_service(ServiceContract::new("notes", "1.0.0"));
    for group in [
        super::notes::registry(),
        super::attachments::registry(),
        super::insights::registry(),
        super::streams::registry(),
    ] {
        for route in group.routes() {
            registry.register(route.clone());
        }
    }
    super::sockets::register_into(&mut registry).expect("static socket contracts");
    registry
}
