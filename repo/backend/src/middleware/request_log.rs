//! Request/outcome logging middleware. Emits `[http][request]` and
//! `[http][response]` lines for every request. Redaction is handled by the
//! logging module's pattern rules; we never include request bodies here.

use actix_service::{Service, Transform};
use actix_web::{
    dev::{ServiceRequest, ServiceResponse},
    Error,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Instant;

use crate::{log_info, log_warn};

pub struct RequestLog;

impl<S, B> Transform<S, ServiceRequest> for RequestLog
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RequestLogMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestLogMiddleware { service: Rc::new(service) }))
    }
}

pub struct RequestLogMiddleware<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for RequestLogMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let srv = self.service.clone();
        let method = req.method().clone();
        let path = req.path().to_string();
        let started = Instant::now();

        Box::pin(async move {
            log_info!("http", "request", "{} {}", method, path);
            let res = srv.call(req).await;
            let elapsed_ms = started.elapsed().as_millis();
            match &res {
                Ok(r) => {
                    let status = r.status().as_u16();
                    if status >= 500 {
                        log_warn!("http", "response", "{} {} -> {} ({}ms)", method, path, status, elapsed_ms);
                    } else {
                        log_info!("http", "response", "{} {} -> {} ({}ms)", method, path, status, elapsed_ms);
                    }
                }
                Err(e) => {
                    log_warn!("http", "response", "{} {} -> error: {} ({}ms)", method, path, e, elapsed_ms);
                }
            }
            res
        })
    }
}
