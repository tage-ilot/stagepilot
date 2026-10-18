//! Asynchronous, typed client for the Planning Center Services API: port of
//! `plugins/planning_center/client.py` (httpx -> reqwest).
//!
//! Same endpoints, query parameters, headers, pagination safety rules and error translation as
//! Python. Error text never contains a response body, URL, header or credential, and nothing in
//! this module logs one.
//!
//! Differences from the Python client (listed in `docs/rust-migration/phase5.md`):
//! * the timeout is one total deadline per request (plus the same value as connect timeout);
//!   httpx applies the value to connect, read, write and pool separately;
//! * a pagination link must have the *same origin* (scheme, host, port) as the API base URL;
//!   Python checks scheme and host only;
//! * redirects are never followed (as in httpx's default), so credentials cannot be forwarded.

use crate::errors::PcoError;
use crate::models::*;
use async_trait::async_trait;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::de::DeserializeOwned;
use stagepilot_core::config::ConnectionMethod;
use stagepilot_core::state::{ServicePlan, Song};
use std::collections::HashSet;
use std::sync::{Arc, Once};
use std::time::Duration as StdDuration;
use url::Url;

pub const API_BASE_URL: &str = "https://api.planningcenteronline.com/";
pub const API_VERSION: &str = "2018-11-01";
pub const SERVICE_TYPES_PATH: &str = "services/v2/service_types";
pub const MAX_PAGE_SIZE: usize = 100;
pub const MAX_PAGES: usize = 100;
pub const DURATION_SOURCE: &str = "Planning Center scheduled item length";

const PERMISSION_WRITE: &str = "Planning Center did not allow this change. The account or token used by StagePilot can't edit this plan.";

/// The operations the plugin, the setup service and the length service need from a client. The
/// real implementation is [`PcoClient`]; tests substitute fakes (Python's
/// `PlanningCenterClientContract`).
#[async_trait]
pub trait PcoApi: Send + Sync {
    async fn list_service_types(&self) -> Result<Vec<ServiceType>, PcoError>;

    async fn load_plan_for_date(
        &self,
        service_type: &ServiceType,
        target_date: NaiveDate,
        timezone_name: &str,
        selected_plan_id: Option<&str>,
        lookahead_days: i64,
    ) -> Result<PlanDiscoveryResult, PcoError>;

    async fn load_plan_for_service_types(
        &self,
        service_types: &[ServiceType],
        target_date: NaiveDate,
        timezone_name: &str,
        selected_plan_id: Option<&str>,
        lookahead_days: i64,
    ) -> Result<PlanDiscoveryResult, PcoError>;

    async fn resolve_selected_plan(
        &self,
        candidates: &[PlanCandidate],
        service_types: &[ServiceType],
        target_date: NaiveDate,
        selected_plan_id: &str,
    ) -> Result<PlanDiscoveryResult, PcoError>;

    async fn get_plan_item(
        &self,
        service_type_id: &str,
        plan_id: &str,
        item_id: &str,
    ) -> Result<ItemResource, PcoError>;

    async fn update_plan_item_length(
        &self,
        service_type_id: &str,
        plan_id: &str,
        item_id: &str,
        length: i64,
        expected_length: Option<i64>,
        write_allowed: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ItemResource, PcoError>;

    async fn close(&self);
}

/// Builds a client from the effective settings (Python's `PlanningCenterClientFactory`).
pub type PcoClientFactory =
    Arc<dyn Fn(&PcoSettings) -> Result<Arc<dyn PcoApi>, PcoError> + Send + Sync>;

/// The factory for the real API, optionally against a different base URL (contract / tests).
pub fn default_factory(base_url: Option<Url>) -> PcoClientFactory {
    Arc::new(move |settings| {
        let client = PcoClient::new(settings, base_url.clone())?;
        Ok(Arc::new(client) as Arc<dyn PcoApi>)
    })
}

enum Auth {
    Basic { app_id: String, secret: String },
    Bearer,
}

pub struct PcoClient {
    http: reqwest::Client,
    base: Url,
    auth: Auth,
}

pub(crate) fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // An error means a provider is already installed process-wide, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl PcoClient {
    pub fn new(settings: &PcoSettings, base_url: Option<Url>) -> Result<Self, PcoError> {
        let not_configured =
            || PcoError::configuration("Planning Center credentials are not configured.");
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("x-pco-api-version"),
            HeaderValue::from_static(API_VERSION),
        );
        let user_agent = HeaderValue::from_str(&settings.user_agent)
            .map_err(|_| PcoError::configuration("The Planning Center user agent is invalid."))?;
        headers.insert(USER_AGENT, user_agent);
        let auth = match settings.connection_method {
            ConnectionMethod::Oauth => {
                let token = match (&settings.access_token, settings.connection_method) {
                    (Some(token), ConnectionMethod::Oauth) => token.expose().to_string(),
                    _ => return Err(not_configured()),
                };
                let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| not_configured())?;
                value.set_sensitive(true);
                headers.insert(AUTHORIZATION, value);
                Auth::Bearer
            }
            ConnectionMethod::Manual => match (&settings.app_id, &settings.secret) {
                (Some(app_id), Some(secret)) => Auth::Basic {
                    app_id: app_id.expose().to_string(),
                    secret: secret.expose().to_string(),
                },
                _ => return Err(not_configured()),
            },
        };
        install_crypto_provider();
        let timeout = StdDuration::from_secs_f64(settings.request_timeout_seconds.clamp(1.0, 60.0));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                PcoError::configuration("The Planning Center client could not be created.")
            })?;
        let base = base_url.unwrap_or_else(|| {
            Url::parse(API_BASE_URL).expect("API_BASE_URL is a valid constant URL")
        });
        Ok(Self { http, base, auth })
    }

    fn builder(&self, method: reqwest::Method, url: Url) -> reqwest::RequestBuilder {
        let request = self.http.request(method, url);
        match &self.auth {
            Auth::Basic { app_id, secret } => request.basic_auth(app_id, Some(secret)),
            Auth::Bearer => request,
        }
    }

    // ---- identifiers ---------------------------------------------------------------------------

    fn validate_identifier(value: &str, label: &str) -> Result<(), PcoError> {
        if is_safe_identifier(value, 128) {
            Ok(())
        } else {
            Err(PcoError::Configuration(format!(
                "The Planning Center {label} identifier is invalid."
            )))
        }
    }

    fn item_path(service_type_id: &str, plan_id: &str, item_id: &str) -> Result<String, PcoError> {
        for (value, label) in [
            (service_type_id, "service type"),
            (plan_id, "plan"),
            (item_id, "item"),
        ] {
            Self::validate_identifier(value, label)?;
        }
        Ok(format!(
            "services/v2/service_types/{service_type_id}/plans/{plan_id}/items/{item_id}"
        ))
    }

    fn join(&self, path: &str) -> Result<Url, PcoError> {
        self.base
            .join(path)
            .map_err(|_| PcoError::response("Planning Center returned an invalid pagination URL."))
    }

    // ---- single item reads and writes ------------------------------------------------------------

    async fn length_request(
        &self,
        method: reqwest::Method,
        path: &str,
        length: Option<i64>,
        expected_length: Option<i64>,
        write_allowed: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ItemResource, PcoError> {
        let is_patch = method == reqwest::Method::PATCH;
        let body = serde_json::json!({"data": {"type": "Item", "attributes": {"length": length}}});
        for attempt in 0..2 {
            if is_patch && !write_allowed() {
                return Err(PcoError::configuration(
                    "Playback started; stop it before changing plan times.",
                ));
            }
            let url = self.join(path)?;
            let mut request = self.builder(method.clone(), url);
            if is_patch {
                request = request.json(&body);
            }
            let response = request.send().await.map_err(|error| {
                if error.is_timeout() {
                    PcoError::Timeout(
                        "Planning Center timed out; the change may not be confirmed.".into(),
                    )
                } else {
                    PcoError::Transport("Could not connect to Planning Center.".into())
                }
            })?;
            let status = response.status().as_u16();
            if status == 401 || status == 403 {
                return Err(PcoError::Permission(PERMISSION_WRITE.into()));
            }
            if status == 429 {
                let delay = parse_retry_after(response.headers().get("retry-after"));
                if let (0, Some(seconds)) = (attempt, delay) {
                    tokio::time::sleep(StdDuration::from_secs(seconds)).await;
                    if is_patch {
                        let current = Box::pin(self.length_request(
                            reqwest::Method::GET,
                            path,
                            None,
                            None,
                            &|| true,
                        ))
                        .await?;
                        let (songs, _) = extract_songs(std::slice::from_ref(&current));
                        if songs.is_empty() || current.attributes.length != expected_length {
                            return Err(PcoError::LengthConflict(
                                "Someone changed this item; their edit was left alone.".into(),
                            ));
                        }
                    }
                    continue;
                }
                return Err(PcoError::RateLimit {
                    retry_after_seconds: delay,
                });
            }
            if !(200..300).contains(&status) {
                return Err(PcoError::Api { status });
            }
            let invalid =
                || PcoError::response("Planning Center returned an invalid item response.");
            let bytes = response.bytes().await.map_err(|_| invalid())?;
            let document: ItemDocument = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            let item = document.data;
            if !item.valid() || item.id != path.rsplit('/').next().unwrap_or_default() {
                return Err(invalid());
            }
            return Ok(item);
        }
        // Both attempts either returned or continued past the single retry.
        Err(PcoError::Unexpected("unreachable".into()))
    }

    // ---- collections -----------------------------------------------------------------------------

    async fn get_collection<T>(
        &self,
        request_path: &str,
        params: Vec<(&'static str, String)>,
        resource_label: &str,
    ) -> Result<Vec<T>, PcoError>
    where
        T: DeserializeOwned + Resource,
    {
        let collection = request_path.to_string();
        let mut request_url = self.join(request_path)?;
        let mut explicit_params: Option<Vec<(&'static str, String)>> = Some(params.clone());
        let mut resources: Vec<T> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for _page in 0..MAX_PAGES {
            let target = with_params(&request_url, explicit_params.as_deref());
            let key = target.to_string();
            if !seen.insert(key) {
                return Err(PcoError::response(
                    "Planning Center returned a repeated pagination request.",
                ));
            }
            let offset = request_offset(&request_url, explicit_params.as_deref())?;
            let document: CollectionDocument<T> =
                self.get_collection_page(target, resource_label).await?;
            let count = document.data.len();
            let next_link = document.links.next.clone().filter(|link| !link.is_empty());
            let next_offset_meta = document.meta.next.as_ref().map(|n| n.offset);
            let total_count = document.meta.total_count;
            resources.extend(document.data);
            if let Some(link) = next_link {
                request_url = self.validated_next_url(&link)?;
                explicit_params = None;
                continue;
            }
            let next_offset = match next_offset_meta {
                Some(next) => Some(next),
                None if total_count.is_some_and(|total| (offset + count as u64) < total)
                    || count == MAX_PAGE_SIZE =>
                {
                    Some(offset + count as u64)
                }
                None => None,
            };
            let Some(next_offset) = next_offset else {
                return Ok(resources);
            };
            if next_offset <= offset {
                return Err(PcoError::response(
                    "Planning Center returned a non-advancing pagination offset.",
                ));
            }
            request_url = self.join(&collection)?;
            let mut next_params: Vec<(&'static str, String)> = params
                .iter()
                .filter(|(k, _)| *k != "per_page" && *k != "offset")
                .cloned()
                .collect();
            next_params.push(("per_page", MAX_PAGE_SIZE.to_string()));
            next_params.push(("offset", next_offset.to_string()));
            explicit_params = Some(next_params);
        }
        Err(PcoError::response(
            "Planning Center pagination exceeded the safety limit.",
        ))
    }

    async fn get_collection_page<T>(
        &self,
        url: Url,
        resource_label: &str,
    ) -> Result<CollectionDocument<T>, PcoError>
    where
        T: DeserializeOwned + Resource,
    {
        let response = self
            .builder(reqwest::Method::GET, url)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    PcoError::Timeout(
                        "Planning Center did not respond before the request timeout.".into(),
                    )
                } else {
                    PcoError::Transport("Could not connect to Planning Center.".into())
                }
            })?;
        let status = response.status().as_u16();
        match status {
            401 => {
                return Err(PcoError::Authentication(
                    "Planning Center rejected the configured application ID or secret.".into(),
                ))
            }
            403 => {
                return Err(PcoError::Permission(
                    "The Planning Center user cannot access the requested Services resource."
                        .into(),
                ))
            }
            429 => {
                return Err(PcoError::RateLimit {
                    retry_after_seconds: parse_retry_after(response.headers().get("retry-after")),
                })
            }
            s if !(200..300).contains(&s) => return Err(PcoError::Api { status: s }),
            _ => {}
        }
        let invalid = || {
            PcoError::Response(format!(
                "Planning Center returned an invalid {resource_label} response."
            ))
        };
        let bytes = response.bytes().await.map_err(|error| {
            if error.is_timeout() {
                PcoError::Timeout(
                    "Planning Center did not respond before the request timeout.".into(),
                )
            } else {
                invalid()
            }
        })?;
        let parsed: Result<CollectionDocument<T>, _> = serde_json::from_slice(&bytes);
        match parsed {
            Ok(document) if document.data.iter().all(Resource::valid) => Ok(document),
            _ => {
                // Never log bodies, validation inputs, messages or URLs.
                tracing::warn!(
                    resource_kind = resource_label,
                    http_status = status,
                    "planning_center_response_validation_diagnostic"
                );
                Err(invalid())
            }
        }
    }

    fn validated_next_url(&self, value: &str) -> Result<Url, PcoError> {
        let url = self.base.join(value).map_err(|_| {
            PcoError::response("Planning Center returned an invalid pagination URL.")
        })?;
        let same_origin = url.scheme() == self.base.scheme()
            && url.host_str() == self.base.host_str()
            && url.port_or_known_default() == self.base.port_or_known_default();
        if !same_origin {
            return Err(PcoError::response(
                "Planning Center returned an unsafe pagination URL.",
            ));
        }
        Ok(url)
    }

    fn ensure_unique_ids<T: Resource>(resources: &[T], label: &str) -> Result<(), PcoError> {
        let ids: HashSet<&str> = resources.iter().map(Resource::resource_id).collect();
        if ids.len() != resources.len() {
            return Err(PcoError::Response(format!(
                "Planning Center returned duplicate {label}."
            )));
        }
        Ok(())
    }

    // ---- service types and plans -------------------------------------------------------------------

    async fn list_plans(
        &self,
        service_type_id: &str,
        target_date: NaiveDate,
        timezone: Tz,
        lookahead_days: i64,
    ) -> Result<Vec<PlanResource>, PcoError> {
        Self::validate_identifier(service_type_id, "service type")?;
        let start = local_midnight(target_date, timezone);
        let end = local_midnight(target_date + Duration::days(lookahead_days + 1), timezone);
        let resources: Vec<PlanResource> = self
            .get_collection(
                &format!("services/v2/service_types/{service_type_id}/plans"),
                vec![
                    ("filter", "after,before".to_string()),
                    ("after", utc_parameter(start - Duration::seconds(1))),
                    ("before", utc_parameter(end)),
                    ("order", "sort_date".to_string()),
                    ("per_page", MAX_PAGE_SIZE.to_string()),
                    ("offset", "0".to_string()),
                ],
                "plan",
            )
            .await?;
        Self::ensure_unique_ids(&resources, "plans")?;
        Ok(resources)
    }

    async fn list_plan_times(
        &self,
        service_type_id: &str,
        plan_id: &str,
    ) -> Result<Vec<PlanTimeResource>, PcoError> {
        Self::validate_identifier(plan_id, "plan")?;
        let resources: Vec<PlanTimeResource> = self
            .get_collection(
                &format!("services/v2/service_types/{service_type_id}/plans/{plan_id}/plan_times"),
                vec![
                    ("where[time_type]", "service".to_string()),
                    ("order", "starts_at".to_string()),
                    ("per_page", MAX_PAGE_SIZE.to_string()),
                    ("offset", "0".to_string()),
                ],
                "plan time",
            )
            .await?;
        Self::ensure_unique_ids(&resources, "plan times")?;
        Ok(resources)
    }

    async fn list_items(
        &self,
        service_type_id: &str,
        plan_id: &str,
    ) -> Result<Vec<ItemResource>, PcoError> {
        let resources: Vec<ItemResource> = self
            .get_collection(
                &format!("services/v2/service_types/{service_type_id}/plans/{plan_id}/items"),
                vec![
                    ("include", "song".to_string()),
                    ("per_page", MAX_PAGE_SIZE.to_string()),
                    ("offset", "0".to_string()),
                ],
                "plan item",
            )
            .await?;
        Self::ensure_unique_ids(&resources, "plan items")?;
        Ok(resources)
    }

    async fn plan_candidates_for_date(
        &self,
        service_type: &ServiceType,
        target_date: NaiveDate,
        timezone_name: &str,
        lookahead_days: i64,
    ) -> Result<Vec<PlanCandidate>, PcoError> {
        if !(0..=365).contains(&lookahead_days) {
            return Err(PcoError::configuration(
                "The Planning Center plan lookahead must be between 0 and 365 days.",
            ));
        }
        let timezone = parse_timezone(timezone_name)?;
        let search_end = target_date + Duration::days(lookahead_days);
        let plans = self
            .list_plans(&service_type.id, target_date, timezone, lookahead_days)
            .await?;
        let mut candidates = Vec::new();
        for plan in &plans {
            let plan_times = self.list_plan_times(&service_type.id, &plan.id).await?;
            let matching = matching_service_times(&plan_times, target_date, search_end, timezone);
            let Some(first) = matching.first() else {
                continue;
            };
            let candidate_date = first.date_naive();
            candidates.push(PlanCandidate {
                id: plan.id.clone(),
                title: plan_title(plan),
                service_type_id: service_type.id.clone(),
                service_type_name: service_type.name.clone(),
                target_date: candidate_date,
                service_times: matching
                    .iter()
                    .copied()
                    .filter(|t| t.date_naive() == candidate_date)
                    .collect(),
            });
        }
        Ok(candidates)
    }

    async fn resolve_plan_candidates(
        &self,
        mut candidates: Vec<PlanCandidate>,
        service_types_by_id: &[(String, ServiceType)],
        target_date: NaiveDate,
        selected_plan_id: Option<&str>,
        not_found_service_type: Option<&ServiceType>,
    ) -> Result<PlanDiscoveryResult, PcoError> {
        if let Some(id) = selected_plan_id {
            Self::validate_identifier(id, "selected plan")?;
        }
        if let Some(nearest) = candidates.iter().map(|c| c.target_date).min() {
            candidates.retain(|c| c.target_date == nearest);
        }
        candidates.sort_by(|a, b| {
            (a.service_times[0], a.id.as_str()).cmp(&(b.service_times[0], b.id.as_str()))
        });
        if candidates.is_empty() {
            return Ok(PlanDiscoveryResult::NotFound {
                service_type: not_found_service_type.cloned(),
                target_date,
            });
        }
        let candidate_date = candidates[0].target_date;
        let selected = match selected_plan_id {
            Some(id) => match candidates.iter().find(|c| c.id == id) {
                Some(found) => found.clone(),
                None => {
                    return Err(PcoError::PlanSelection(
                        "The selected Planning Center plan does not match a candidate on the \
                         nearest available service date."
                            .into(),
                    ))
                }
            },
            None if candidates.len() > 1 => {
                return Ok(PlanDiscoveryResult::Ambiguous {
                    service_type: not_found_service_type.cloned(),
                    target_date: candidate_date,
                    candidates,
                })
            }
            None => candidates[0].clone(),
        };
        let service_type = service_types_by_id
            .iter()
            .find(|(id, _)| *id == selected.service_type_id)
            .map(|(_, value)| value)
            .ok_or_else(|| PcoError::Unexpected("unknown service type".into()))?;
        let items = self.list_items(&service_type.id, &selected.id).await?;
        let (songs, skipped_items) = extract_songs(&items);
        let plan = ServicePlan {
            id: selected.id.clone(),
            title: selected.title.clone(),
            date: selected.target_date,
            service_type: service_type.name.clone(),
            service_type_id: Some(service_type.id.clone()),
            service_times: selected
                .service_times
                .iter()
                .map(|t| t.format("%H:%M").to_string())
                .collect(),
            duration_source: DURATION_SOURCE.to_string(),
            songs,
        };
        Ok(PlanDiscoveryResult::Loaded {
            candidate: selected,
            plan,
            skipped_items,
        })
    }
}

#[async_trait]
impl PcoApi for PcoClient {
    async fn list_service_types(&self) -> Result<Vec<ServiceType>, PcoError> {
        let resources: Vec<ServiceTypeResource> = self
            .get_collection(
                SERVICE_TYPES_PATH,
                vec![
                    ("order", "sequence".to_string()),
                    ("per_page", MAX_PAGE_SIZE.to_string()),
                    ("offset", "0".to_string()),
                ],
                "service type",
            )
            .await?;
        Self::ensure_unique_ids(&resources, "service types")?;
        Ok(resources
            .into_iter()
            .map(|r| ServiceType {
                id: r.id,
                name: r.attributes.name,
                sequence: u32::try_from(r.attributes.sequence).unwrap_or(u32::MAX),
                archived: r.attributes.archived_at.is_some() || r.attributes.deleted_at.is_some(),
            })
            .collect())
    }

    async fn load_plan_for_date(
        &self,
        service_type: &ServiceType,
        target_date: NaiveDate,
        timezone_name: &str,
        selected_plan_id: Option<&str>,
        lookahead_days: i64,
    ) -> Result<PlanDiscoveryResult, PcoError> {
        let candidates = self
            .plan_candidates_for_date(service_type, target_date, timezone_name, lookahead_days)
            .await?;
        self.resolve_plan_candidates(
            candidates,
            &[(service_type.id.clone(), service_type.clone())],
            target_date,
            selected_plan_id,
            Some(service_type),
        )
        .await
    }

    /// One service type's failure must not discard every other service type's valid candidates;
    /// only when every type fails is the first failure raised.
    async fn load_plan_for_service_types(
        &self,
        service_types: &[ServiceType],
        target_date: NaiveDate,
        timezone_name: &str,
        selected_plan_id: Option<&str>,
        lookahead_days: i64,
    ) -> Result<PlanDiscoveryResult, PcoError> {
        let mut candidates = Vec::new();
        let mut failures: Vec<PcoError> = Vec::new();
        for service_type in service_types {
            match self
                .plan_candidates_for_date(service_type, target_date, timezone_name, lookahead_days)
                .await
            {
                Ok(found) => candidates.extend(found),
                Err(error) if error.is_planning_center_error() => {
                    tracing::warn!(
                        exception_type = error.type_name(),
                        http_status = error.http_status(),
                        "planning_center_service_type_load_failed"
                    );
                    failures.push(error);
                }
                Err(error) => return Err(error),
            }
        }
        if !failures.is_empty() && failures.len() == service_types.len() {
            return Err(failures.remove(0));
        }
        let by_id: Vec<(String, ServiceType)> = service_types
            .iter()
            .map(|s| (s.id.clone(), s.clone()))
            .collect();
        self.resolve_plan_candidates(candidates, &by_id, target_date, selected_plan_id, None)
            .await
    }

    async fn resolve_selected_plan(
        &self,
        candidates: &[PlanCandidate],
        service_types: &[ServiceType],
        target_date: NaiveDate,
        selected_plan_id: &str,
    ) -> Result<PlanDiscoveryResult, PcoError> {
        let by_id: Vec<(String, ServiceType)> = service_types
            .iter()
            .map(|s| (s.id.clone(), s.clone()))
            .collect();
        self.resolve_plan_candidates(
            candidates.to_vec(),
            &by_id,
            target_date,
            Some(selected_plan_id),
            None,
        )
        .await
    }

    async fn get_plan_item(
        &self,
        service_type_id: &str,
        plan_id: &str,
        item_id: &str,
    ) -> Result<ItemResource, PcoError> {
        let path = Self::item_path(service_type_id, plan_id, item_id)?;
        self.length_request(reqwest::Method::GET, &path, None, None, &|| true)
            .await
    }

    async fn update_plan_item_length(
        &self,
        service_type_id: &str,
        plan_id: &str,
        item_id: &str,
        length: i64,
        expected_length: Option<i64>,
        write_allowed: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<ItemResource, PcoError> {
        if !(0..=86_400).contains(&length) {
            return Err(PcoError::configuration("Invalid plan item length."));
        }
        let path = Self::item_path(service_type_id, plan_id, item_id)?;
        self.length_request(
            reqwest::Method::PATCH,
            &path,
            Some(length),
            expected_length,
            write_allowed,
        )
        .await
    }

    async fn close(&self) {}
}

// ---- helpers (module level so tests can reach them) --------------------------------------------------

fn with_params(url: &Url, params: Option<&[(&'static str, String)]>) -> Url {
    let Some(params) = params else {
        return url.clone();
    };
    let mut merged = url.clone();
    // `copy_merge_params`: a name in `params` replaces the same name already in the URL.
    let existing: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !params.iter().any(|(name, _)| *name == k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    merged.set_query(None);
    {
        let mut pairs = merged.query_pairs_mut();
        for (k, v) in &existing {
            pairs.append_pair(k, v);
        }
        for (k, v) in params {
            pairs.append_pair(k, v);
        }
    }
    merged
}

fn request_offset(url: &Url, params: Option<&[(&'static str, String)]>) -> Result<u64, PcoError> {
    let invalid = || PcoError::response("Planning Center returned an invalid pagination offset.");
    let raw = match params {
        Some(params) => params
            .iter()
            .find(|(k, _)| *k == "offset")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "0".to_string()),
        None => url
            .query_pairs()
            .find(|(k, _)| k == "offset")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "0".to_string()),
    };
    raw.trim()
        .parse::<i64>()
        .map_err(|_| invalid())
        .and_then(|v| u64::try_from(v).map_err(|_| invalid()))
}

/// `Retry-After` as non-negative integer seconds; anything else (including HTTP dates) is absent.
pub fn parse_retry_after(value: Option<&HeaderValue>) -> Option<u64> {
    let text = value?.to_str().ok()?;
    let parsed: i64 = text.trim().parse().ok()?;
    u64::try_from(parsed).ok()
}

pub fn parse_timezone(name: &str) -> Result<Tz, PcoError> {
    name.parse::<Tz>()
        .map_err(|_| PcoError::configuration("The configured Planning Center timezone is invalid."))
}

fn local_midnight(date: NaiveDate, timezone: Tz) -> DateTime<Utc> {
    let naive = date.and_time(NaiveTime::MIN);
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(first, _) => first.with_timezone(&Utc),
        // A nonexistent local midnight (a DST gap at 00:00): read it with the offset in force
        // just before the gap, which is what zoneinfo's `fold=0` does.
        chrono::LocalResult::None => {
            let before = naive - Duration::hours(3);
            let offset = match timezone.from_local_datetime(&before) {
                chrono::LocalResult::Single(t) => t.offset().fix(),
                chrono::LocalResult::Ambiguous(first, _) => first.offset().fix(),
                chrono::LocalResult::None => utc_offset(),
            };
            offset
                .from_local_datetime(&naive)
                .single()
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
        }
    }
}

fn utc_offset() -> FixedOffset {
    FixedOffset::east_opt(0).expect("zero is a valid UTC offset")
}

use chrono::Offset;

fn utc_parameter(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn plan_title(resource: &PlanResource) -> String {
    let title = resource.attributes.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let dates = resource.attributes.dates.trim();
    if !dates.is_empty() {
        return dates.to_string();
    }
    format!("Plan {}", resource.id)
}

/// Service times (in `timezone`) whose local date is within `[start_date, end_date]`, sorted.
pub fn matching_service_times(
    resources: &[PlanTimeResource],
    start_date: NaiveDate,
    end_date: NaiveDate,
    timezone: Tz,
) -> Vec<DateTime<FixedOffset>> {
    let mut matching: Vec<DateTime<FixedOffset>> = resources
        .iter()
        .filter(|r| r.attributes.time_type.to_lowercase() == "service")
        .map(|r| {
            r.attributes
                .starts_at
                .with_timezone(&timezone)
                .fixed_offset()
        })
        .filter(|t| start_date <= t.date_naive() && t.date_naive() <= end_date)
        .collect();
    matching.sort();
    matching
}

/// Ordered songs and the visible explanation for each skipped item.
pub fn extract_songs(resources: &[ItemResource]) -> (Vec<Song>, Vec<SkippedPlanItem>) {
    let mut ordered: Vec<(usize, &ItemResource)> = resources.iter().enumerate().collect();
    ordered.sort_by_key(|(index, r)| (r.attributes.sequence, *index));
    let mut songs: Vec<Song> = Vec::new();
    let mut skipped: Vec<SkippedPlanItem> = Vec::new();
    for (_, resource) in ordered {
        let attributes = &resource.attributes;
        let item_type = attributes.item_type.to_lowercase();
        let title = attributes.title.trim().to_string();
        let source_song = resource.relationships.song.data.as_ref();
        let reason = if item_type == "header" {
            Some(SkippedItemReason::Header)
        } else if item_type == "media" {
            Some(SkippedItemReason::Media)
        } else if item_type != "song" && source_song.is_none() {
            Some(SkippedItemReason::NotSong)
        } else if title.is_empty() {
            Some(SkippedItemReason::MissingTitle)
        } else {
            None
        };
        let description = attributes
            .description
            .as_deref()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty());
        let length = attributes
            .length
            .map(|l| u32::try_from(l).unwrap_or(u32::MAX));
        let sequence = u32::try_from(attributes.sequence).unwrap_or(u32::MAX);
        if let Some(reason) = reason {
            skipped.push(SkippedPlanItem {
                item_id: resource.id.clone(),
                title: if title.is_empty() {
                    "(untitled)".to_string()
                } else {
                    title
                },
                description,
                item_type: attributes.item_type.clone(),
                sequence,
                duration_seconds: length,
                reason,
            });
            continue;
        }
        let mut song = Song::new(&resource.id, &title, length, songs.len() as u32 + 1);
        song.service_sequence = Some(sequence);
        song.is_generic = source_song.is_none();
        song.source_song_id = source_song.map(|s| s.id.clone());
        song.description = description;
        songs.push(song);
    }
    (songs, skipped)
}
