use axum::{
    body::Body,
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{
    App,
    model::audit_log::{self, AuditAction, AuditResourceType, NewAuditLog},
};

#[derive(Clone, Debug)]
pub struct AuditEvent {
    pub action: AuditAction,
    pub resource_type: AuditResourceType,
    pub resource_id: Uuid,
    pub resource_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub trait AuditEventExt: IntoResponse + Sized {
    fn with_audit_event(self, event: AuditEvent) -> Response {
        let mut response = self.into_response();
        response.extensions_mut().insert(event);
        response
    }
}

impl<T> AuditEventExt for T where T: IntoResponse {}

pub async fn record_http_audit_log(
    State(env): State<App>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let response = next.run(request).await;

    if !response.status().is_success() {
        return response;
    }

    if let Some(event) = response.extensions().get::<AuditEvent>().cloned() {
        audit_log::record_best_effort(
            env.pool(),
            NewAuditLog {
                action: event.action,
                resource_type: event.resource_type,
                resource_id: event.resource_id,
                resource_name: event.resource_name,
                metadata: event.metadata,
            },
        )
        .await;
    }

    response
}
