use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, OnceLock};

use toolkit::Gear;
use toolkit::context::GearCtx;
use toolkit::contracts::{OpenApiRegistry, RestApiCapability};

use crate::domain::local_client::NodesRegistryLocalClient;
use crate::domain::service::Service;
use nodes_registry_sdk::NodesRegistryClient;

#[toolkit::gear(
    name = "nodes-registry",
    capabilities = [rest],
    client = nodes_registry_sdk::NodesRegistryClient
)]
pub struct NodesRegistry {
    service: OnceLock<Arc<Service>>,
}

impl Default for NodesRegistry {
    fn default() -> Self {
        Self {
            service: OnceLock::new(),
        }
    }
}

#[async_trait]
impl Gear for NodesRegistry {
    async fn init(&self, ctx: &GearCtx) -> Result<()> {
        // Create the service
        let service = Arc::new(Service::new());
        self.service
            .set(service.clone())
            .map_err(|_| anyhow::anyhow!("{} gear already initialized", Self::MODULE_NAME))?;

        // Expose the client to the ClientHub
        let api: Arc<dyn NodesRegistryClient> = Arc::new(NodesRegistryLocalClient::new(service));
        ctx.client_hub().register::<dyn NodesRegistryClient>(api);

        Ok(())
    }
}

impl RestApiCapability for NodesRegistry {
    fn register_rest(
        &self,
        _ctx: &GearCtx,
        router: axum::Router,
        openapi: &dyn OpenApiRegistry,
    ) -> Result<axum::Router> {
        let service = self
            .service
            .get()
            .ok_or_else(|| anyhow::anyhow!("Service not initialized"))?
            .clone();

        let router = crate::api::rest::routes::register_routes(router, openapi, service);

        tracing::info!("Nodes registry REST routes registered");
        Ok(router)
    }
}
