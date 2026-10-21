//! Cloudflare control-plane adapter (`remote_provider.py`).

use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{RemoteError, Res};
use crate::files::ControlConfig;
use crate::http::HttpClient;

pub trait TunnelProvider {
    /// Returns (tunnel id, run token).
    fn ensure(&self, name: &str, hostname: &str, port: u16) -> Res<(String, String)>;
    fn revoke(&self, name: &str, hostname: &str) -> Res<()>;
}

pub struct CloudflareProvider {
    client: HttpClient,
    tunnels: String,
    records: String,
}

fn perr(message: &str) -> RemoteError {
    RemoteError::provider(message)
}

/// Python truthiness of an optional JSON value.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn str_field<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

impl CloudflareProvider {
    pub fn new(config: &ControlConfig, client: HttpClient) -> Self {
        Self {
            client,
            tunnels: format!("/accounts/{}/cfd_tunnel", config.account_id),
            records: format!("/zones/{}/dns_records", config.zone_id),
        }
    }

    /// `Ok(None)` for HTTP 404, the `result` member otherwise.
    fn request(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Res<Option<Value>> {
        let unavailable = || perr("Provider response unavailable or invalid; retry reconciliation");
        let query: Vec<(String, String)> = query
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let response = self
            .client
            .request(method, path, &[], &query, body)
            .map_err(|_| unavailable())?;
        if response.status == 404 {
            return Ok(None);
        }
        if !response.is_success() {
            return Err(RemoteError::provider(format!(
                "Provider request failed (HTTP {})",
                response.status
            )));
        }
        let data = response.json().map_err(|_| unavailable())?;
        match data.get("success") {
            Some(Value::Bool(true)) => {}
            _ if data.is_object() => return Err(perr("Provider rejected request")),
            _ => return Err(unavailable()),
        }
        data.get("result")
            .cloned()
            .map(Some)
            .ok_or_else(unavailable)
    }

    fn tunnel(&self, name: &str) -> Res<Option<Value>> {
        let rows = self.request(
            "GET",
            &self.tunnels,
            &[("name", name), ("is_deleted", "false")],
            None,
        )?;
        let Some(Value::Array(rows)) = rows else {
            return Err(perr("Cannot enumerate installation tunnel"));
        };
        let mut rows: Vec<&Value> = rows
            .iter()
            .filter(|r| str_field(r, "name") == Some(name) && !truthy(r.get("deleted_at")))
            .collect();
        if rows.len() > 1 {
            return Err(perr("Ambiguous installation tunnel; refusing changes"));
        }
        let Some(row) = rows.pop() else {
            return Ok(None);
        };
        if !str_field(row, "id").is_some_and(|id| Uuid::parse_str(id).is_ok()) {
            return Err(perr("Invalid tunnel identity"));
        }
        if str_field(row, "config_src") != Some("cloudflare") {
            return Err(perr("Refusing to adopt a locally managed tunnel"));
        }
        Ok(Some(row.clone()))
    }

    fn dns(&self, hostname: &str) -> Res<Option<Value>> {
        match self.request("GET", &self.records, &[("name", hostname)], None)? {
            Some(Value::Array(rows)) if rows.len() <= 1 => Ok(rows.into_iter().next()),
            _ => Err(perr("Ambiguous hostname; refusing changes")),
        }
    }

    fn owned(record: &Value, name: &str, hostname: &str, tunnel_id: &str) -> bool {
        str_field(record, "comment") == Some(name)
            && str_field(record, "name") == Some(hostname)
            && str_field(record, "type") == Some("CNAME")
            && str_field(record, "content") == Some(&format!("{tunnel_id}.cfargotunnel.com"))
            && record.get("proxied") == Some(&Value::Bool(true))
    }

    fn configuration(&self, tunnel_id: &str, ingress: Value) -> Res<()> {
        let path = format!("{}/{tunnel_id}/configurations", self.tunnels);
        let config = json!({"ingress": ingress, "warp-routing": {"enabled": false}});
        self.request("PUT", &path, &[], Some(json!({"config": config})))?;
        let actual = self.request("GET", &path, &[], None)?;
        let Some(actual @ Value::Object(_)) = actual else {
            return Err(perr("Tunnel route read-back did not match"));
        };
        if actual.pointer("/config/ingress") != Some(&ingress) {
            return Err(perr("Tunnel route read-back did not match"));
        }
        if actual.pointer("/config/warp-routing/enabled") != Some(&Value::Bool(false)) {
            return Err(perr("Private network routing must be disabled"));
        }
        Ok(())
    }
}

impl TunnelProvider for CloudflareProvider {
    fn ensure(&self, name: &str, hostname: &str, port: u16) -> Res<(String, String)> {
        let existing = self.dns(hostname)?;
        let mut tunnel = self.tunnel(name)?;
        if let Some(record) = &existing {
            let id = tunnel.as_ref().and_then(|t| str_field(t, "id"));
            if !id.is_some_and(|id| Self::owned(record, name, hostname, id)) {
                return Err(perr("Hostname is owned by another route; refusing changes"));
            }
        }
        if tunnel.is_none() {
            // Never blindly retry POST after a lost response: the next invocation reconciles the
            // durable random installation/generation name first.
            self.request(
                "POST",
                &self.tunnels,
                &[],
                Some(json!({"name": name, "config_src": "cloudflare"})),
            )?;
            tunnel = self.tunnel(name)?;
            if tunnel.is_none() {
                return Err(perr("Tunnel creation not confirmed"));
            }
        }
        let tunnel_id = tunnel
            .as_ref()
            .and_then(|t| str_field(t, "id"))
            .unwrap_or_default()
            .to_owned();
        self.configuration(
            &tunnel_id,
            json!([
                {"hostname": hostname, "service": format!("http://127.0.0.1:{port}")},
                {"service": "http_status:404"},
            ]),
        )?;
        if existing.is_none() {
            self.request(
                "POST",
                &self.records,
                &[],
                Some(json!({
                    "type": "CNAME",
                    "name": hostname,
                    "content": format!("{tunnel_id}.cfargotunnel.com"),
                    "proxied": true,
                    "ttl": 1,
                    "comment": name,
                })),
            )?;
        }
        match self.dns(hostname)? {
            Some(actual) if Self::owned(&actual, name, hostname, &tunnel_id) => {}
            _ => return Err(perr("Hostname route read-back did not match")),
        }
        let token = self.request(
            "GET",
            &format!("{}/{tunnel_id}/token", self.tunnels),
            &[],
            None,
        )?;
        match token {
            Some(Value::String(t)) if !t.is_empty() && !t.chars().any(char::is_whitespace) => {
                Ok((tunnel_id, t))
            }
            _ => Err(perr("Installation credential unavailable")),
        }
    }

    fn revoke(&self, name: &str, hostname: &str) -> Res<()> {
        let tunnel = self.tunnel(name)?;
        let record = self.dns(hostname)?;
        if let Some(record) = record {
            let id = tunnel.as_ref().and_then(|t| str_field(t, "id"));
            if !id.is_some_and(|id| Self::owned(&record, name, hostname, id)) {
                return Err(perr("Refusing to delete an unowned hostname"));
            }
            let record_id = str_field(&record, "id").unwrap_or_default();
            self.request(
                "DELETE",
                &format!("{}/{record_id}", self.records),
                &[],
                None,
            )?;
            if self.dns(hostname)?.is_some() {
                return Err(perr("Hostname removal not confirmed"));
            }
        }
        if let Some(tunnel) = tunnel {
            let tunnel_id = str_field(&tunnel, "id").unwrap_or_default().to_owned();
            // Block routes before forcing connections closed. Deleting the tunnel retires its run
            // credential; do not confuse stop with revocation.
            self.configuration(&tunnel_id, json!([{"service": "http_status:404"}]))?;
            self.request(
                "DELETE",
                &format!("{}/{tunnel_id}/connections", self.tunnels),
                &[],
                None,
            )?;
            self.request(
                "DELETE",
                &format!("{}/{tunnel_id}", self.tunnels),
                &[],
                None,
            )?;
            if self.tunnel(name)?.is_some() {
                return Err(perr("Tunnel revocation not confirmed"));
            }
        }
        Ok(())
    }
}
