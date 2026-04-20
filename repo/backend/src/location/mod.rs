pub mod geocode_stub;
pub mod routes;

pub fn scope() -> actix_web::Scope {
    actix_web::web::scope("/api/location").service(routes::geocode)
}
