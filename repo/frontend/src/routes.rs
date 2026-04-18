use uuid::Uuid;
use yew_router::prelude::*;

#[derive(Clone, Routable, PartialEq, Debug)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/login")]
    Login,
    #[at("/dashboard")]
    Dashboard,
    #[at("/work-orders/:id")]
    WorkOrder { id: Uuid },
    #[at("/work-orders/:id/steps/:step_id")]
    RecipeStep { id: Uuid, step_id: Uuid },
    #[at("/work-orders/:id/map")]
    MapView { id: Uuid },
    #[at("/notifications")]
    Notifications,
    #[at("/analytics")]
    Analytics,
    #[at("/admin")]
    Admin,
    #[not_found]
    #[at("/404")]
    NotFound,
}
