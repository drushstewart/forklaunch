{{#modules}}pub mod {{module}};
{{/modules}}
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::RouteContract;

pub fn contracts() -> Vec<RouteContract> {
    std::iter::empty()
{{#modules}}        .chain({{module}}::contracts())
{{/modules}}        .collect()
}

pub fn register_router(router: ForkLaunchRouter) -> ForkLaunchRouter {
{{#modules}}    let router = {{module}}::register(router);
{{/modules}}    router
}
