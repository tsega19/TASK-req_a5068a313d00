//! Pagination envelope used by every list endpoint.
//! Response shape: `{ "data": [...], "page": 1, "per_page": 20, "total": N }`

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PageParams {
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub per_page: Option<u32>,
}

impl PageParams {
    pub fn effective(self) -> (u32, u32) {
        let page = self.page.unwrap_or(1).max(1);
        let per_page = self.per_page.unwrap_or(20).clamp(1, 200);
        (page, per_page)
    }

    pub fn offset_limit(self) -> (i64, i64) {
        let (page, per_page) = self.effective();
        (((page - 1) * per_page) as i64, per_page as i64)
    }
}

#[derive(Debug, Serialize)]
pub struct Paginated<T: Serialize> {
    pub data: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

impl<T: Serialize> Paginated<T> {
    pub fn new(data: Vec<T>, params: PageParams, total: i64) -> Self {
        let (page, per_page) = params.effective();
        Self { data, page, per_page, total }
    }
}
